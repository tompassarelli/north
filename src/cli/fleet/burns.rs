use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};
use serde_json::json;

use crate::fleet::model::Role;
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;
use crate::spend::burn::{self, Burn, Sample};
use crate::spend::{BURNS, Rules};
use crate::threads::calibration::{self, Priced};
use crate::usage::decide::world::Rates;
use crate::usage::{open_ledger, profiles};
use crate::watch;

const SAMPLED_WITHIN_S: f64 = 30.0 * 60.0;

pub(super) fn watch(now_ms: u64) -> Vec<String> {
    let Ok(db) = open_ledger(&profiles::home(), None, &|| {}) else { return Vec::new() };
    let now = now_ms as f64 / 1000.0;
    let rules = Rules::configured();
    let samples = match window_samples(&db, "session", now - rules.five_hour_burn_min * 60.0) {
        Ok(samples) => samples,
        Err(why) => return vec![format!("the burn governor's burn was not measured: the usage table is unreadable: {why}")],
    };
    let week_samples = match window_samples(&db, "weekly_all", now - rules.week_burn_h * 3600.0) {
        Ok(samples) => samples,
        Err(why) => return vec![format!("the burn governor's burn was not measured: the usage table is unreadable: {why}")],
    };
    if !samples.iter().any(|(_, sample)| now - sample.at <= SAMPLED_WITHIN_S) {
        return Vec::new();
    }
    let agents = watch::state_home().join("agents");
    let accounts: std::collections::BTreeSet<&str> = samples.iter().map(|(account, _)| account.as_str()).collect();
    let keyed = crate::platform::orchestration::read().map(|cfg| crate::spend::keyed(&cfg)).unwrap_or_default();
    let measured = accounts.into_iter().filter(|account| !keyed.iter().any(|name| account.strip_prefix(name.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))));
    let burns: Vec<Burn> = measured
        .map(|account| {
            let of = |readings: &[(String, Sample)]| readings.iter().filter(|(name, _)| name == account).map(|(_, sample)| *sample).collect::<Vec<Sample>>();
            let per_week = priced_rates(&db, account, now_ms).ok().flatten().map_or(0.0, |rates| rates.five_hour.points_per_usd() / rates.week.points_per_usd());
            burn::burn(&rules, account, &of(&samples), &of(&week_samples), if per_week.is_finite() { per_week } else { 0.0 }, now)
        })
        .collect();
    match write_json(&agents.join(BURNS), &json!({ "burns": burns })) {
        Ok(()) => Vec::new(),
        Err(why) => vec![format!("the spend burn governor's burn readings were not written, so spawners project the last ones: {why}")],
    }
}

/// Every account's readings of the usage-table window `name` (`session` is the five-hour window, `weekly_all` the week) since `since`.
fn window_samples(db: &Connection, name: &str, since: f64) -> rusqlite::Result<Vec<(String, Sample)>> {
    let seconds = |text: &str| Timestamp::parse(text).ok().map(|at| at.unix_millis() as f64 / 1000.0);
    let since = Timestamp::from_unix_millis((since * 1000.0) as i64).iso();
    let present: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage'", [], |row| row.get(0))?;
    if present == 0 {
        return Ok(Vec::new());
    }
    let mut query = db.prepare("SELECT account, ts, used_pct, resets_at FROM usage WHERE provider = 'claude' AND kind = 'window' AND name = ?1 AND used_pct IS NOT NULL AND ts >= ?2")?;
    let rows = query.query_map(params![name, since], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?, row.get::<_, Option<String>>(3)?)))?;
    Ok(rows.flatten().filter_map(|(account, ts, used_pct, resets_at)| Some((format!("claude:{account}"), Sample { at: seconds(&ts)?, used_pct, resets_at: resets_at.as_deref().and_then(seconds) }))).collect())
}

/// The rates that price `account`'s spend: none when unmeasured, and why when the newest are stale.
fn priced_rates(db: &Connection, account: &str, now_ms: u64) -> Result<Option<Rates>, String> {
    let label = account.strip_prefix("claude:").unwrap_or(account);
    match calibration::priced(db, label, Timestamp::from_unix_millis(now_ms as i64)) {
        Ok(Priced::Measured(rates)) => Ok(Some(rates)),
        Ok(Priced::Stale(why)) => Err(why),
        Ok(Priced::Unmeasured) | Err(_) => Ok(None),
    }
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let pending = path.with_extension("json.tmp");
    fs::write(&pending, serde_json::to_vec(value).map_err(|error| error.to_string())?).map_err(|error| format!("{}: {error}", pending.display()))?;
    fs::rename(&pending, path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Whether the spend rule admits one more session on the request's own account: what a tick's hand-over checks before it starts a session elsewhere.
pub(super) fn spend_admits(fleet: &Fleet, request: &crate::fleet::model::LaunchRequest) -> crate::error::NorthResult<()> {
    use crate::error::NorthError;
    let starter = if request.role == Some(Role::Proxy) { crate::spend::Starter::Reserved } else { crate::spend::Starter::Worker };
    let (view, rules) = crate::dispatch::spend_now(&fleet.roster()).map_err(|why| NorthError::Saturated(format!("spend view unread: {why}")))?;
    let account = format!("{}:{}", request.provider.label(), request.account);
    view.admit(&rules, &account, starter, crate::spend::Placed::default()).map_err(|refusal| NorthError::Saturated(refusal.to_string()))
}
