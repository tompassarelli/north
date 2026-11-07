//! `north fleet reports --json`: the entries dispatch reads of this registry's sessions retired in the last two days, in the shape of
//! `show --json`, which the laptop mirrors to the host that runs dispatch, since no host there
//! reaches the laptop's archive. A retired session's transcript no longer changes, so each is read once and kept beside it.
use std::fs;

use serde_json::{Map, Value, json};

use crate::error::NorthResult;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;

use super::turn_json::turn_json;
use super::usage::usage;

const KEPT_MS: u64 = 48 * 60 * 60 * 1000;
const TOOL_CHARS: usize = 400;
const MIRRORED: &str = "mirror-report-read.json";

pub(super) fn run(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    if flags != ["--json"] {
        return Err(usage());
    }
    let mut found = Map::new();
    for name in fleet.retired_names_since(now_ms().saturating_sub(KEPT_MS)) {
        if let Some(report) = report(fleet, &name) {
            found.insert(name, report);
        }
    }
    println!("{}", Value::Object(found));
    Ok(())
}

fn report(fleet: &Fleet, name: &str) -> Option<Value> {
    let (dir, _) = fleet.retired_session(name)?;
    let kept = dir.join(MIRRORED);
    if let Some(read) = fs::read_to_string(&kept).ok().and_then(|text| serde_json::from_str(&text).ok()) {
        return Some(read);
    }
    let follower = fleet.follow_retired(name).ok()?;
    let transcript = follower.transcript();
    let said: Vec<(String, String)> = transcript.entries.iter().map(|entry| (entry.kind.label().to_owned(), entry.text.clone())).collect();
    let entries: Vec<Value> = crate::dispatch::entries_read(&said)
        .into_iter()
        .map(|(kind, text)| json!({"kind": kind, "text": if kind == "tool" { text.chars().take(TOOL_CHARS).collect() } else { text }}))
        .collect();
    let session = json!({"name": name, "entries": entries, "turns": transcript.turns.iter().map(turn_json).collect::<Vec<_>>()});
    let _ = fs::write(&kept, session.to_string());
    Some(session)
}
