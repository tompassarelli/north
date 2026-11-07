//! `north fleet reports --json`: the transcripts of this registry's sessions retired in the last two days, in the shape of
//! `show --json` with tool commands cut short, which the laptop mirrors to the host that runs dispatch, since no host there
//! reaches the laptop's archive. A retired session's transcript no longer changes, so each is read once and kept beside it.
use std::fs;

use serde_json::{Map, Value, json};

use crate::error::NorthResult;
use crate::fleet::model::EntryKind;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;

use super::show::turn_json;
use super::usage::usage;

const KEPT_MS: u64 = 48 * 60 * 60 * 1000;
const TOOL_CHARS: usize = 400;
const MIRRORED: &str = "mirror-report.json";

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
    let entries: Vec<Value> = transcript
        .entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::User | EntryKind::Assistant | EntryKind::Tool | EntryKind::Notice))
        .map(|entry| {
            let text = if entry.kind == EntryKind::Tool { entry.text.chars().take(TOOL_CHARS).collect() } else { entry.text.clone() };
            json!({"kind": entry.kind.label(), "text": text})
        })
        .collect();
    let session = json!({"name": name, "entries": entries, "turns": transcript.turns.iter().map(turn_json).collect::<Vec<_>>()});
    let _ = fs::write(&kept, session.to_string());
    Some(session)
}
