//! A usage sampler that has not read a provider for ten minutes: every pace reader is on stale numbers,
//! so the proxy hears once per episode, naming the provider and its last good reading.

use serde_json::Value;

use super::alerts::{Action, Node, Record, State, deliver};
use super::host::{Host, Places};
use crate::foundation::clock::Timestamp;
use crate::pace::window::STALE_SECONDS;

const STATE_FILE: &str = "sampler.json";

#[derive(Clone, Debug, PartialEq)]
pub struct Failing {
    pub provider: String,
    pub since: Option<f64>,
    pub why: String,
}

/// The providers in pace-control.json whose last read failed or was throttled.
pub fn failing(pace: &Value) -> Vec<Failing> {
    let Some(providers) = pace["providers"].as_object() else { return Vec::new() };
    providers
        .iter()
        .filter(|(_, read)| read["code"].as_i64() != Some(0) || read["throttled"] == true)
        .map(|(provider, read)| {
            let code = read["code"].as_i64().map_or_else(|| "no exit code".to_owned(), |code| format!("exit {code}"));
            let why = read["error"].as_str().map_or(code.clone(), |error| format!("{code}: {error}"));
            Failing { provider: provider.clone(), since: read["sampled_at"].as_f64(), why: if read["throttled"] == true { format!("throttled, {why}") } else { why } }
        })
        .collect()
}

fn reading(since: Option<f64>) -> String {
    since.map_or_else(|| "none recorded".to_owned(), |at| format!("{}Z", &Timestamp::from_unix_seconds_f64(at).iso_minute()[11..16]))
}

/// One alert per provider once it has gone ten minutes without a good read; a good read ends the episode.
pub fn sampler_plan(failing: &[Failing], previous: &State, now: f64) -> (State, Vec<Action>) {
    let mut state: State = previous.iter().filter(|(provider, _)| failing.iter().any(|f| &f.provider == *provider)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut actions = Vec::new();
    for failure in failing {
        let record = state.entry(failure.provider.clone()).or_insert_with(|| Record { since: failure.since.unwrap_or(now), ..Record::default() });
        let stale = now - record.since;
        let key = format!("sampler:{}:fleet:proxy", failure.provider);
        if stale >= STALE_SECONDS && !record.sent.contains_key(&key) {
            let text = format!(
                "Usage sampler for {} has not read for {:.0}m ({}); last good reading {}. Every pace reader is on stale usage until it reads; run north pace sample on this host and fix the read",
                failure.provider,
                stale / 60.0,
                failure.why,
                reading(failure.since)
            );
            actions.push(Action { repo: failure.provider.clone(), key, node: "proxy".into(), channel: "fleet", text });
        }
    }
    (state, actions)
}

pub fn pace_control(places: &Places) -> Value {
    std::fs::read_to_string(places.state.join("agents/pace-control.json")).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

pub fn sampler_alert(places: &Places, host: &dyn Host, now: f64) -> Result<(), String> {
    let failing = failing(&pace_control(places));
    deliver(places, host, STATE_FILE, |previous, _: &[Node]| sampler_plan(&failing, previous, now), now)
}
