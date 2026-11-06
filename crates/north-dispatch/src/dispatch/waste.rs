//! The runs of the last day that spent usage and finished nothing: final reports that stop for another host, launches that ran beside a holder, and revert tickets the proxy withdrew as false.
use super::fleets::remote_json;
use super::stops;
use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};
use serde_json::Value;
use std::time::Duration;

pub const WINDOW: &str = "24h";
const WINDOW_S: f64 = 24.0 * 3600.0;
const WITHDRAWN: &str = "lander-withdrawn";

pub type Count = Result<usize, String>;

#[derive(Clone, Debug, PartialEq)]
pub struct Waste {
    pub stop_host: Count,
    pub duplicate_launches: Count,
    pub false_reverts: Count,
}

impl Waste {
    pub fn line(&self) -> String {
        let part = |count: &Count, singular: &str, plural: &str| match count {
            Ok(1) => format!("1 {singular}"),
            Ok(count) => format!("{count} {plural}"),
            Err(_) => format!("{plural} unmeasured"),
        };
        let parts = [part(&self.stop_host, "Stop: host", "Stop: host"), part(&self.duplicate_launches, "duplicate launch", "duplicate launches"), part(&self.false_reverts, "false revert", "false reverts")];
        let unread: Vec<String> = [("Stop: host", &self.stop_host), ("duplicate launches", &self.duplicate_launches), ("false reverts", &self.false_reverts)]
            .into_iter()
            .filter_map(|(kind, count)| count.as_ref().err().map(|why| format!("{kind}: {why}")))
            .collect();
        let tail = if unread.is_empty() { String::new() } else { format!(" ({})", unread.join("; ")) };
        format!("waste in {WINDOW}: {}{tail}", parts.join(", "))
    }

    pub fn json(&self) -> Value {
        let number = |count: &Count| count.as_ref().ok().map_or(Value::Null, |count| Value::from(*count));
        let unmeasured: serde_json::Map<String, Value> = [("stop_host", &self.stop_host), ("duplicate_launches", &self.duplicate_launches), ("false_reverts", &self.false_reverts)]
            .into_iter()
            .filter_map(|(kind, count)| count.as_ref().err().map(|why| (kind.to_owned(), Value::from(why.as_str()))))
            .collect();
        serde_json::json!({ "window": WINDOW, "stop_host": number(&self.stop_host), "duplicate_launches": number(&self.duplicate_launches), "false_reverts": number(&self.false_reverts), "unmeasured": unmeasured })
    }
}

pub fn measure() -> Waste {
    let now = Timestamp::now();
    let since = Timestamp::from_unix_seconds(now.unix_seconds() - WINDOW_S as i64).iso();
    Waste { stop_host: stop_host(&since), duplicate_launches: duplicate_launches(now.unix_seconds() as f64), false_reverts: false_reverts(&since) }
}

fn ledger(query: &str) -> Result<Vec<Value>, String> {
    sqlite::rows(&crate::ledger::threads_database(), Access::ReadOnly, Duration::from_secs(5), query).map_err(|error| error.to_string())
}

fn stop_host(since: &str) -> Count {
    let rows = ledger(&format!("SELECT report FROM runs WHERE role = 'worker' AND report LIKE '%Stop:%' AND ended >= '{since}'"))?;
    Ok(rows.iter().filter(|row| row["report"].as_str().is_some_and(|report| stops::host_need(report).is_some())).count())
}

fn false_reverts(since: &str) -> Count {
    let rows = ledger(&format!("SELECT subject FROM incidents WHERE class = '{WITHDRAWN}' AND detected_at >= '{since}'"))?;
    Ok(rows.iter().filter_map(|row| row["subject"].as_str()).filter(|subject| subject.starts_with("revert-") || subject.contains("-revert-")).count())
}

fn duplicate_launches(at: f64) -> Count {
    let listing = remote_json(None, &["fleet", "list", "--json", "--all"])?;
    Ok(duplicates(&listing, at))
}

pub fn duplicates(listing: &Value, at: f64) -> usize {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| session["placement"].as_str().is_some_and(|note| note.contains("dup: ")))
        .filter(|session| session["created_ms"].as_f64().or_else(|| session["seen_ms"].as_f64()).is_some_and(|ms| at - ms / 1000.0 < WINDOW_S))
        .count()
}
