use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use crate::fleet::model::{Delivery, Role};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::PROXY;
use crate::fleet::sender::Sender;
use crate::foundation::clock::Timestamp;
use crate::spend::tripwire::{self, SPENDERS, Sample, Spenders, TRIP_LOG, Trip};
use crate::spend::{Rules, TRIPS};
use crate::threads::calibration::{self, Priced};
use crate::usage::{open_ledger, profiles};
use crate::watch;

const SAMPLED_WITHIN_S: f64 = 30.0 * 60.0;
const STALE_INPUTS: &str = "spend/stale-inputs.json";

pub(super) fn watch(fleet: &Fleet, now_ms: u64) -> Vec<String> {
    let Ok(db) = open_ledger(&profiles::home(), None, &|| {}) else { return Vec::new() };
    let now = now_ms as f64 / 1000.0;
    let rules = Rules::configured();
    let span = rules.fleet_trip.minutes.max(rules.session_trip.minutes) * 60.0;
    let samples = match five_hour_samples(&db, now - span) {
        Ok(samples) => samples,
        Err(why) => return vec![format!("the spend tripwire did not run: the usage table is unreadable: {why}")],
    };
    if !samples.iter().any(|(_, sample)| now - sample.at <= SAMPLED_WITHIN_S) {
        return Vec::new();
    }
    let agents = watch::state_home().join("agents");
    let read: Vec<(String, String, f64)> = running(fleet).into_iter().filter_map(|(name, account)| Some((name.clone(), account, fleet.spend_of(&name).ok()??.api_usd?))).collect();
    let spenders = read_json::<Spenders>(&agents.join(SPENDERS)).record(&read, now, span);
    let mut lines = Vec::new();
    if let Err(why) = write_json(&agents.join(SPENDERS), &spenders) {
        lines.push(format!("the spend tripwire's session spend was not kept, so the next tick measures sessions from then: {why}"));
    }
    let accounts: std::collections::BTreeSet<&str> = samples.iter().map(|(account, _)| account.as_str()).chain(spenders.sessions.iter().map(|spender| spender.account.as_str())).collect();
    let keyed = crate::platform::orchestration::read().map(|cfg| crate::spend::keyed(&cfg)).unwrap_or_default();
    let judged_accounts: Vec<&str> = accounts.into_iter().filter(|account| !keyed.iter().any(|name| account.strip_prefix(name.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with(':')))).collect();
    let priced: Vec<(&str, Result<f64, String>)> = judged_accounts.iter().map(|account| (*account, points_per_usd(&db, account, now_ms))).collect();
    let confirmed: Vec<Trip> = priced
        .iter()
        .filter_map(|(account, rate)| {
            let own: Vec<Sample> = samples.iter().filter(|(name, _)| name == account).map(|(_, sample)| *sample).collect();
            tripwire::trip(&rules, account, &own, &spenders.sessions, *rate.as_ref().unwrap_or(&0.0), now)
        })
        .collect();
    lines.extend(stale_alarm(fleet, &agents, &priced));
    let previous: Vec<Trip> = serde_json::from_value(read_json::<Value>(&agents.join(TRIPS))["trips"].clone()).unwrap_or_default();
    let judged = tripwire::judge(&previous, confirmed, now, &rules, |account| points_per_usd(&db, account, now_ms).unwrap_or(0.0));
    if let Err(why) = append(&agents.join(TRIP_LOG), &judged.new) {
        lines.push(format!("the spend tripwire's new trips were not logged: {why}"));
    }
    if let Err(why) = write_json(&agents.join(TRIPS), &json!({ "trips": judged.current })) {
        lines.push(format!("the spend tripwire's trips were not written, so spawners read the last ones: {why}"));
    }
    let proxy_here = fleet.session(PROXY).is_ok();
    for trip in &judged.new {
        let text = format!("spend tripwire: no new worker starts on {} until the burst falls out of its window: {}", trip.account, trip.line);
        if !proxy_here {
            lines.push(text);
            continue;
        }
        let told = fleet.send_signed(&Sender::fleet(), PROXY, &text, Delivery::Queued);
        lines.push(told.map_or_else(|error| format!("{text} (not told to the proxy: {})", error.user_message()), |()| format!("{text} (proxy told)")));
    }
    lines
}

fn running(fleet: &Fleet) -> Vec<(String, String)> {
    let sessions = fleet.sessions().unwrap_or_default();
    sessions.into_iter().filter(|record| record.provider.label() == "claude" && !record.verification && record.role() != Role::Proxy && fleet.running(record)).map(|record| (record.name, format!("claude:{}", record.account))).collect()
}

fn five_hour_samples(db: &Connection, since: f64) -> rusqlite::Result<Vec<(String, Sample)>> {
    let seconds = |text: &str| Timestamp::parse(text).ok().map(|at| at.unix_millis() as f64 / 1000.0);
    let since = Timestamp::from_unix_millis((since * 1000.0) as i64).iso();
    let present: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage'", [], |row| row.get(0))?;
    if present == 0 {
        return Ok(Vec::new());
    }
    let mut query = db.prepare("SELECT account, ts, used_pct, resets_at FROM usage WHERE provider = 'claude' AND kind = 'window' AND name = 'session' AND used_pct IS NOT NULL AND ts >= ?1")?;
    let rows = query.query_map(params![since], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?, row.get::<_, Option<String>>(3)?)))?;
    Ok(rows.flatten().filter_map(|(account, ts, used_pct, resets_at)| Some((format!("claude:{account}"), Sample { at: seconds(&ts)?, used_pct, resets_at: resets_at.as_deref().and_then(seconds) }))).collect())
}

/// The five-hour points per dollar the session rule prices `account`'s spend with: zero when unmeasured, so the rule refuses nothing, and why when the newest rate is stale.
fn points_per_usd(db: &Connection, account: &str, now_ms: u64) -> Result<f64, String> {
    let label = account.strip_prefix("claude:").unwrap_or(account);
    match calibration::priced(db, label, Timestamp::from_unix_millis(now_ms as i64)) {
        Ok(Priced::Measured(rates)) => Ok(rates.five_hour.points_per_usd()),
        Ok(Priced::Stale(why)) => Err(why),
        Ok(Priced::Unmeasured) | Err(_) => Ok(0.0),
    }
}

/// The text the proxy gets once when the session rule's rate input on `accounts` is stale, so the rule refuses nothing on it.
fn stale_text(why: &str, accounts: &[&str]) -> String {
    format!("spend tripwire: its session rule refuses nothing on {} until the rate is remeasured, and the worker-turn rule there falls back to its assumed charge: {why}", accounts.join(", "))
}

/// Tells the proxy once per stale rate row the guards met, kept in `STALE_INPUTS` until the row is replaced.
fn stale_alarm(fleet: &Fleet, agents: &Path, priced: &[(&str, Result<f64, String>)]) -> Vec<String> {
    let mut stale: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for (account, rate) in priced {
        if let Err(why) = rate {
            stale.entry(why.as_str()).or_default().push(account);
        }
    }
    let told: Vec<String> = read_json(&agents.join(STALE_INPUTS));
    let mut lines = Vec::new();
    let mut kept = Vec::new();
    for (why, accounts) in stale {
        if told.iter().any(|earlier| earlier == why) {
            kept.push(why.to_owned());
            continue;
        }
        let text = stale_text(why, &accounts);
        if fleet.session(PROXY).is_err() {
            kept.push(why.to_owned());
            lines.push(text);
            continue;
        }
        let sent = fleet.send_signed(&Sender::fleet(), PROXY, &text, Delivery::Now);
        if sent.is_ok() {
            kept.push(why.to_owned());
        }
        lines.push(sent.map_or_else(|error| format!("{text} (not told to the proxy: {})", error.user_message()), |()| format!("{text} (proxy told)")));
    }
    if kept != told
        && let Err(why) = write_json(&agents.join(STALE_INPUTS), &kept)
    {
        lines.push(format!("the spend tripwire's stale-input alarms were not kept, so the next tick may tell the proxy again: {why}"));
    }
    lines
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let pending = path.with_extension("json.tmp");
    fs::write(&pending, serde_json::to_vec(value).map_err(|error| error.to_string())?).map_err(|error| format!("{}: {error}", pending.display()))?;
    fs::rename(&pending, path).map_err(|error| format!("{}: {error}", path.display()))
}

fn append(path: &Path, trips: &[Trip]) -> Result<(), String> {
    if trips.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    for trip in trips {
        writeln!(file, "{}", serde_json::to_string(trip).map_err(|error| error.to_string())?).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}
