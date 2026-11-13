//! The worker-ledger rows of this host's DeepSeek sessions (north#1020): one finished run per opencode or pi fleet
//! session, read from the turns its session host recorded, and the landings of its commits by its Sonnet reviewer or
//! by the review job. Without them the threads ledger held no DeepSeek run, so `north ledger opencode` reported none.
use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::fleet::landings::Push;
use crate::foundation::clock::Timestamp;
use crate::ledger::Run;
use crate::providers::claude::runs::row::eta;
use crate::providers::ledger_rows::{self as rows, Shared};

pub const PROVIDERS: [&str; 2] = ["opencode", "pi"];
const LIVE: [&str; 2] = ["starting", "working"];

/// What the collector reads for this host's DeepSeek sessions.
pub struct Sources<'a> {
    pub turns: &'a [Run],
    pub prompts: &'a HashMap<String, String>,
    pub nodes: Option<&'a [Value]>,
    pub pushes: &'a [Push],
    /// The commits a review verdict names for each session.
    pub reviewed: &'a HashMap<String, Vec<String>>,
}

/// The run row of each finished DeepSeek session since the cutoff, each followed by a push_join row per landing of its
/// commits by a session other than its own.
pub fn rows(shared: &Shared, sources: &Sources, cutoff: f64) -> Vec<Value> {
    let mut sessions: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    for turn in sources.turns.iter().filter(|turn| PROVIDERS.contains(&turn.provider.as_str())) {
        sessions.entry(turn.session.as_str()).or_default().push(turn);
    }
    let mut out = Vec::new();
    for (name, turns) in sessions {
        let skipped = shared.known.contains(name) || !asked(shared.session, sources.nodes, name) || live(sources.nodes, name);
        let Some((row, started_ms, ended_ms)) = (!skipped).then(|| run_row(shared, name, &turns, sources.prompts.get(name).map_or("", String::as_str))).flatten() else { continue };
        if (ended_ms as f64) / 1000.0 >= cutoff {
            out.push(row);
            out.extend(landings(name, started_ms, sources));
        }
    }
    out
}

fn run_row(shared: &Shared, name: &str, turns: &[&Run], brief: &str) -> Option<(Value, i64, i64)> {
    let first = turns.iter().min_by_key(|turn| turn.started_ms)?;
    let last = turns.iter().max_by_key(|turn| turn.ended_ms)?;
    let (started, ended) = (Timestamp::from_unix_millis(first.started_ms).iso(), Timestamp::from_unix_millis(last.ended_ms).iso());
    let model = last.model.clone().unwrap_or_default();
    let wall_min = (last.ended_ms - first.started_ms).max(0) as f64 / 60_000.0;
    let active_min = (turns.iter().map(|turn| turn.wall_ms.max(0)).sum::<i64>() as f64 / 60_000.0).min(wall_min);
    let tenths = |minutes: f64| (minutes * 10.0).round() / 10.0;
    let effort = last.effort.clone().unwrap_or_else(|| "default".to_owned());
    let row = json!({
        "id": name,
        "fleet_name": name,
        "role": "worker",
        "provider": first.provider,
        "account": last.account,
        "model": model,
        "effort": effort,
        "fast": 0,
        "tier": format!("{} {model} {effort}", first.provider),
        "brief": rows::fields(brief),
        "eta_min": eta(brief),
        "started": started,
        "staffed": started,
        "ended": ended,
        "review_requested": ended,
        "status": "finished",
        "actual_min": rows::minutes(first.started_ms as f64 / 1000.0, last.ended_ms as f64 / 1000.0),
        "tokens": turns.iter().map(|turn| turn.tokens_out).sum::<i64>(),
        "peak_ctx": turns.iter().map(|turn| turn.tokens_in + turn.tokens_cache_read).max().unwrap_or(0),
        "turns": turns.iter().map(|turn| turn.turns).sum::<i64>(),
        "tool_calls": turns.iter().map(|turn| turn.tool_calls).sum::<Option<i64>>(),
        "compactions": turns.iter().map(|turn| turn.compactions).sum::<Option<i64>>(),
        "cost_usd": turns.iter().map(|turn| turn.cost_usd).sum::<Option<f64>>(),
        "active_min": tenths(active_min),
        "wait_min": tenths(wall_min - active_min),
        "brief_text": (!brief.is_empty()).then_some(brief),
        "brief_origin": (!brief.is_empty()).then(|| format!("{}.prompt", first.provider)),
        "outcome": rows::ENDED,
        "report": "",
        "backfill": shared.recorded.contains(name),
    });
    Some((row, first.started_ms, last.ended_ms))
}

/// The landings of `name`'s commits by its Sonnet reviewer or by a push of a commit its verdicts name, as push_join rows.
fn landings(name: &str, started_ms: i64, sources: &Sources) -> Vec<Value> {
    let reviewer = format!("{name}-rv");
    let commits = sources.reviewed.get(name).map_or(&[][..], Vec::as_slice);
    sources
        .pushes
        .iter()
        .filter(|push| !push.landed.is_empty() && push.at_ms as i64 >= started_ms && (push.session == reviewer || commits.contains(&push.oid)))
        .map(|push| json!({"push_join": 1, "fleet_name": name, "landed_sha": push.landed, "landed_at": Timestamp::from_unix_millis(push.at_ms as i64).iso()}))
        .collect()
}

fn asked(session: &str, nodes: Option<&[Value]>, name: &str) -> bool {
    session.is_empty() || session == name || nodes.into_iter().flatten().any(|node| node["name"] == name && node["conversation"] == session)
}

fn live(nodes: Option<&[Value]>, name: &str) -> bool {
    nodes.into_iter().flatten().any(|node| node["name"] == name && node["status"].as_str().is_some_and(|status| LIVE.contains(&status)))
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
