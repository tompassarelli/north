//! The land branches Autoland refused: a tip whose `autoland` status is failure or error stays on
//! origin until someone pushes a fix. The session that pushed it hears the refusal from safe-push's
//! wait; when that session is gone, the stall alert names the branch to the Fleet parent and the proxy.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::alerts::Node;
use super::autoland::failed_since;
use super::host::Places;

/// The session that pushed each sha of the repository, from safe-push's push log.
pub(crate) fn pushers(places: &Places, repo: &str) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(places.state.join("safe-push/pushes.tsv")).unwrap_or_default();
    text.lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|fields| fields.len() > 5 && fields[1] == repo && !fields[5].is_empty())
        .map(|fields| (fields[3].to_owned(), fields[5].to_owned()))
        .collect()
}

/// Each refused lane with its branch, pusher, reason, run link and minutes since the refusal.
pub(crate) fn rows(tips: &BTreeMap<String, (f64, String)>, states: &BTreeMap<String, Value>, dead: &BTreeSet<&String>, pushers: &BTreeMap<String, String>, now: f64) -> Value {
    let rows = dead
        .iter()
        .filter_map(|sha| {
            let state = states.get(*sha)?;
            Some(json!({
                "branch": tips.get(*sha)?.1,
                "session": pushers.get(*sha),
                "reason": state["description"].as_str().unwrap_or_default(),
                "url": state["targetUrl"].as_str().unwrap_or_default(),
                "minutes": (now - failed_since(states, sha)?).max(0.0) / 60.0,
            }))
        })
        .collect();
    Value::Array(rows)
}

/// `claude/land-x (pushed by s, rebasing onto main conflicts in a.rs, URL, 40m)` for each refused
/// lane whose pusher is not a live Fleet session.
pub(crate) fn lines(row: &Value, nodes: &[Node]) -> Vec<String> {
    row["refused"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|lane| lane["session"].as_str().is_none_or(|session| !nodes.iter().any(|node| node.active && node.id == session)))
        .map(|lane| {
            let pusher = lane["session"].as_str().map_or_else(|| "pushed outside safe-push".to_owned(), |session| format!("pushed by {session}"));
            let details = [lane["reason"].as_str().unwrap_or_default(), lane["url"].as_str().unwrap_or_default()].into_iter().filter(|text| !text.is_empty()).collect::<Vec<_>>().join(", ");
            format!("{} ({pusher}{}{}, {:.0}m)", lane["branch"].as_str().unwrap_or_default(), if details.is_empty() { "" } else { ", " }, details, lane["minutes"].as_f64().unwrap_or(0.0))
        })
        .collect()
}
