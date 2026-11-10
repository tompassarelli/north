//! The organization's own account of what its API work spent: one hourly usage report priced by model, and
//! the daily cost report the provider posts a day later, read with an Admin key this host holds. The
//! difference between the two is logged, and the grants in `claude-accounts.toml` minus the cost report's
//! posted spend become the organization's provider reading, which outranks north's list-price estimate.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use north_pace::credit::Grant;
use north_pace::foundation::clock::Timestamp;
use north_pace::router::pricing::{PriceTable, TokenUse};
use serde::Deserialize;
use serde_json::Value;

use super::api_probe::curl_config;

pub const BASE: &str = "https://api.anthropic.com";
/// Where this host mounts each organization's Admin key, named `anthropic-<org>-admin-key`.
pub const KEY_DIR: &str = "/run/secrets";
pub const EVERY: Duration = Duration::from_secs(600);

const USAGE: &str = "/v1/organizations/usage_report/messages";
const COST: &str = "/v1/organizations/cost_report";
/// How long one Admin report request may take: a week of hourly buckets is a large answer the provider
/// sometimes serves after six seconds (2026-11-10: `curl exit 28: Operation timed out after 6001
/// milliseconds with 0 bytes received`), so each request gets enough room to finish.
const ATTEMPT: Duration = Duration::from_secs(15);
const DAYS_BACK: i64 = 31;
const PAGES: usize = 40;
/// The most hourly buckets the usage report answers in one page: the provider refuses `limit` over 168
/// with `bucket_width=1h` (a week), so an older window is read in pages, not in one request.
const USAGE_PAGE_HOURS: usize = 168;

/// A report reader: the path and query it asks for, answered with the provider's JSON or why it could not read it.
type Fetch<'a> = dyn Fn(&str, &[(&str, String)]) -> Result<Value, String> + 'a;

/// A page of either report: one bucket per hour or day, each with the rows that name its tokens or cost.
#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    data: Vec<Bucket>,
    #[serde(default)]
    next_page: Option<String>,
}

#[derive(Deserialize)]
struct Bucket {
    starting_at: String,
    #[serde(default)]
    results: Vec<Row>,
}

#[derive(Deserialize, Default)]
struct Row {
    #[serde(default)]
    uncached_input_tokens: i64,
    #[serde(default)]
    cache_creation: CacheCreation,
    #[serde(default)]
    cache_read_input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    amount: Option<Value>,
}

#[derive(Deserialize, Default)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_1h_input_tokens: i64,
    #[serde(default)]
    ephemeral_5m_input_tokens: i64,
}

/// One organization's grants, the day each was granted, and where its Admin key is read from.
struct Pooled {
    name: String,
    id: String,
    key: PathBuf,
}

/// Every api organization the policy names with an Admin key under `key_dir`, in policy order.
fn pooled(policy: &Path, key_dir: &Path) -> Vec<Pooled> {
    north_pace::credit::orgs(policy)
        .into_iter()
        .filter_map(|(name, id)| {
            let key = key_dir.join(format!("anthropic-{name}-admin-key"));
            key.is_file().then_some(Pooled { name, id, key })
        })
        .collect()
}

/// Polls every keyed organization, records the provider reading for each and says what it read. Called by
/// `north fleet accounts --poll` and the fleet tick every ten minutes; prints nothing when no key exists.
pub fn poll(policy: &Path, state: &Path, key_dir: &Path, now: Timestamp) -> Vec<String> {
    let pooled = pooled(policy, key_dir);
    if pooled.is_empty() {
        return Vec::new();
    }
    let prices = match PriceTable::load() {
        Ok(prices) => prices,
        Err(error) => return vec![format!("api readings unread: the price table does not parse: {error}")],
    };
    let mut lines = Vec::new();
    for org in &pooled {
        let fetch = |path: &str, params: &[(&str, String)]| fetch_at(BASE, ATTEMPT, path, &org.key, params);
        lines.extend(poll_org(policy, state, org, &prices, now, &fetch));
    }
    lines
}

/// Polls the keyed organizations only when the last poll is `every` old, and stamps the attempt so a host
/// with no live key does not retry on every tick.
pub fn poll_due(policy: &Path, state: &Path, key_dir: &Path, every: Duration, now: Timestamp) -> Vec<String> {
    let stamp = state.join("north/api-poll-at");
    let last = std::fs::read_to_string(&stamp).ok().and_then(|text| text.trim().parse::<i64>().ok());
    if last.is_some_and(|last| now.unix_seconds() - last < every.as_secs() as i64) {
        return Vec::new();
    }
    let lines = poll(policy, state, key_dir, now);
    if !lines.is_empty() {
        if let Some(parent) = stamp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(stamp, now.unix_seconds().to_string());
    }
    lines
}

fn poll_org(
    policy: &Path,
    state: &Path,
    org: &Pooled,
    prices: &PriceTable,
    now: Timestamp,
    fetch: &Fetch<'_>,
) -> Vec<String> {
    let grants = north_pace::credit::grants(policy).remove(&org.id).unwrap_or_default();
    if grants.is_empty() {
        return vec![format!("api:{} has no declared grants, so there is no credit to read", org.name)];
    }
    let today = now.iso_date();
    let start = window_start(&grants, now);
    let usage = match usage_days(fetch, &start, now, prices) {
        Ok(days) => days,
        Err(error) => return vec![format!("api:{} usage report unread: {error}", org.name)],
    };
    let cost = match cost_days(fetch, &start, now) {
        Ok(days) => days,
        Err(error) => return vec![format!("api:{} cost report unread: {error}", org.name)],
    };
    let (spends, spend_usd) = posted_spend(&cost);
    let left = left_of(&grants, &spends, &today);
    let reading = north_pace::credit::readings::Reading { usd: left, at: now.unix_seconds(), source: north_pace::credit::readings::Source::Provider };
    let wrote = north_pace::credit::readings::record(&north_pace::credit::readings::directory(state), &org.id, &reading).unwrap_or(false);
    let kept = if wrote { "" } else { ", kept the newer reading" };
    let cross = cross_check(&usage.priced, &cost);
    let unpriced = if usage.unpriced.is_empty() { String::new() } else { format!("; unpriced {}", usage.unpriced.join(", ")) };
    vec![format!(
        "api:{} reads ${left:.2} left from the Admin API at {}{kept} (spend ${spend_usd:.2} of ${:.2} granted; usage priced ${:.2}{cross}{unpriced})",
        org.name,
        now.iso_minute(),
        grants.iter().map(|grant| grant.usd).sum::<f64>(),
        usage.priced.values().sum::<f64>()
    )]
}

/// The day a poll starts from: the earliest grant still unexpired, else the earliest grant, and never more
/// than the 31 days the Admin API keeps.
fn window_start(grants: &[Grant], now: Timestamp) -> String {
    let today = now.iso_date();
    let earliest = |live: bool| {
        grants
            .iter()
            .filter(|grant| !grant.granted.is_empty() && (!live || grant.expires >= today))
            .map(|grant| grant.granted.clone())
            .min()
    };
    let floor = Timestamp::from_unix_seconds(now.unix_seconds() - DAYS_BACK * 86_400).iso_date();
    earliest(true).or_else(|| earliest(false)).filter(|day| *day >= floor).unwrap_or(floor)
}

/// The price of each UTC day's usage report rows, and every model the price table does not know.
struct Usage {
    priced: BTreeMap<String, f64>,
    unpriced: Vec<String>,
}

fn usage_days(fetch: &Fetch<'_>, start: &str, now: Timestamp, prices: &PriceTable) -> Result<Usage, String> {
    let mut params = vec![
        ("starting_at", format!("{start}T00:00:00Z")),
        ("ending_at", now.iso()),
        ("bucket_width", "1h".to_owned()),
        ("group_by[]", "model".to_owned()),
        ("limit", USAGE_PAGE_HOURS.to_string()),
    ];
    let mut usage = Usage { priced: BTreeMap::new(), unpriced: Vec::new() };
    for _ in 0..PAGES {
        let page: Page = serde_json::from_value(fetch(USAGE, &params)?).map_err(|error| format!("usage report does not parse: {error}"))?;
        for bucket in page.data {
            price_bucket(bucket, prices, &mut usage);
        }
        match page.next_page {
            Some(next) => {
                params.retain(|(name, _)| *name != "page");
                params.push(("page", next));
            }
            None => return Ok(usage),
        }
    }
    Err("usage report did not reach its last page".into())
}

/// One hourly bucket's rows: each priced model adds its tokens to that UTC day, and a model the table
/// does not know is named once so the reading says so instead of silently counting it at zero.
fn price_bucket(bucket: Bucket, prices: &PriceTable, usage: &mut Usage) {
    let day = bucket.starting_at.get(..10).unwrap_or_default().to_owned();
    for row in bucket.results {
        let Some(model) = row.model.as_deref() else { continue };
        match prices.price(model) {
            Some(price) => {
                let tokens = TokenUse {
                    input: row.uncached_input_tokens,
                    output: row.output_tokens,
                    cache_read: row.cache_read_input_tokens,
                    cache_write: row.cache_creation.ephemeral_1h_input_tokens + row.cache_creation.ephemeral_5m_input_tokens,
                };
                *usage.priced.entry(day.clone()).or_default() += price.of(tokens);
            }
            None if !usage.unpriced.iter().any(|seen| seen == model) => usage.unpriced.push(model.to_owned()),
            None => {}
        }
    }
}

fn cost_days(fetch: &Fetch<'_>, start: &str, now: Timestamp) -> Result<BTreeMap<String, f64>, String> {
    let mut params = vec![
        ("starting_at", format!("{start}T00:00:00Z")),
        ("ending_at", now.iso()),
        ("bucket_width", "1d".to_owned()),
        ("limit", "31".to_owned()),
    ];
    let mut days = BTreeMap::new();
    for _ in 0..PAGES {
        let page: Page = serde_json::from_value(fetch(COST, &params)?).map_err(|error| format!("cost report does not parse: {error}"))?;
        for bucket in page.data {
            let day = bucket.starting_at.get(..10).unwrap_or_default().to_owned();
            let mut cents = 0.0;
            for row in bucket.results {
                cents += amount_cents(row.amount)?;
            }
            *days.entry(day).or_default() += cents / 100.0;
        }
        match page.next_page {
            Some(next) => {
                params.retain(|(name, _)| *name != "page");
                params.push(("page", next));
            }
            None => return Ok(days),
        }
    }
    Err("cost report did not reach its last page".into())
}

fn amount_cents(amount: Option<Value>) -> Result<f64, String> {
    match amount {
        Some(Value::String(text)) => text.parse().map_err(|_| format!("cost report amount {text:?} is not a number")),
        Some(Value::Number(number)) => number.as_f64().ok_or_else(|| "cost report amount is not a number".to_owned()),
        Some(other) => Err(format!("cost report amount {other} is not a number")),
        None => Ok(0.0),
    }
}

/// Each day's spend in dollars as the provider's cost report posts it: a day the report has not posted is not
/// charged, because north's priced usage is an estimate and the reading must be the provider's figure.
fn posted_spend(cost: &BTreeMap<String, f64>) -> (Vec<(String, f64)>, f64) {
    let spends: Vec<(String, f64)> = cost.iter().map(|(day, usd)| (day.clone(), *usd)).collect();
    let total = spends.iter().map(|(_, usd)| *usd).sum();
    (spends, total)
}

/// The credit still left on the grants unexpired `today`, after charging each day's spend to the grants
/// granted on or before that day and unexpired then.
fn left_of(grants: &[Grant], spends: &[(String, f64)], today: &str) -> f64 {
    north_pace::credit::left_after(grants, spends).into_iter().zip(grants).filter(|(_, grant)| grant.expires.as_str() >= today).map(|(left, _)| left).sum::<f64>().max(0.0)
}

/// The total the cost report and the priced usage report disagree on over the days both cover.
fn cross_check(priced: &BTreeMap<String, f64>, cost: &BTreeMap<String, f64>) -> String {
    let both: Vec<(&String, f64, f64)> = cost.iter().filter_map(|(day, usd)| priced.get(day).map(|priced| (day, *usd, *priced))).collect();
    if both.is_empty() {
        return String::new();
    }
    let (cost_usd, priced_usd) = both.iter().fold((0.0, 0.0), |(cost, priced), (_, there, here)| (cost + there, priced + here));
    let days = if both.len() == 1 { "day" } else { "days" };
    format!(", cost report ${cost_usd:.2} on {} posted {days} ({:+.2})", both.len(), cost_usd - priced_usd)
}

fn fetch_at(base: &str, limit: Duration, path: &str, key: &Path, params: &[(&str, String)]) -> Result<Value, String> {
    let key = std::fs::read_to_string(key).map_err(|error| format!("key file: {error}"))?;
    let config = curl_config(key.trim())?;
    let limit = limit.as_secs_f64().to_string();
    let mut command = Command::new("curl");
    command.args(["--silent", "--show-error", "--connect-timeout", &limit, "--max-time", &limit, "--config", "-", "--get", "--write-out", "\n%{http_code}"]);
    for (name, value) in params {
        command.args(["--data-urlencode", &format!("{name}={value}")]);
    }
    command.arg(format!("{base}{path}"));
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("curl: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes()).map_err(|error| format!("curl: {error}"))?;
    }
    let output = child.wait_with_output().map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("curl exit {}: {}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, status) = text.rsplit_once('\n').ok_or("curl answered no status")?;
    let status: u16 = status.trim().parse().map_err(|_| format!("curl answered a status of {status:?}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("the Admin API answered {status}: {}", body.trim()));
    }
    serde_json::from_str(body).map_err(|error| format!("the Admin API answered unreadable JSON: {error}"))
}

#[cfg(test)]
mod tests;
