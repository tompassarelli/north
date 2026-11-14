use std::fs;
use std::path::Path;

use north_base::platform::host_load::short;
use serde::Deserialize;

use crate::snapshot::Rollout;

#[derive(Deserialize)]
struct Event {
    event: String,
    #[serde(default)]
    sha: String,
    #[serde(default)]
    host: Option<String>,
    at: u64,
}

#[derive(Deserialize)]
struct Promote {
    sha: String,
    state: String,
    at: u64,
}

pub fn read(state: &Path, at: u64) -> Result<Rollout, String> {
    let path = state.join("runtime-pins.jsonl");
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let events: Vec<Event> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).filter(|event: &Event| event.at <= at).collect();
    let mut rollout = Rollout::default();
    for event in &events {
        let state = match event.event.as_str() {
            "promoted" => "live",
            "rolledBack" => "rolled back",
            "paged" => "paged",
            "waiting" => "waiting",
            _ => continue,
        };
        rollout.promote_state = Some(state.into());
        rollout.promote_sha = Some(short(&event.sha));
        rollout.promote_at = Some(event.at);
    }
    let rollbacks: Vec<&Event> = events.iter().filter(|event| event.event == "rolledBack").collect();
    rollout.rollbacks_24h = rollbacks.iter().filter(|event| event.at + 86_400 > at).count() as u32;
    rollout.last_rollback = rollbacks.last().map(|event| (event.host.clone().unwrap_or_default(), event.at));
    let promote = fs::read_to_string(state.join("runtime-promote.json")).ok().and_then(|text| serde_json::from_str::<Promote>(&text).ok());
    if let Some(promote) = promote.filter(|promote| promote.at <= at && rollout.promote_at.is_none_or(|seen| promote.at >= seen)) {
        rollout.promote_state = Some(promote.state);
        rollout.promote_sha = Some(short(&promote.sha));
        rollout.promote_at = Some(promote.at);
    }
    Ok(rollout)
}
