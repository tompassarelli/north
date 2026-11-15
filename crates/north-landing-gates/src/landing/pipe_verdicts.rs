//! Whether hand-ins and after-landing runs reach verdicts, read from this host's hand-in refusals and the lander's
//! after-landing ledgers: the one reading behind the land watch's no-verdict page and the sheriff's revert of a
//! landing-path commit that stops the pipe (north#1537).
use std::fs;
use std::path::Path;

use serde_json::Value;

const REFUSALS: &str = "north/enqueue-refusals.jsonl";
const LAND_QUEUES: &str = "north/land-queue";
const LANDER_LEDGER: &str = "lander-post-land.tsv";
const NO_VERDICT: &str = "reached no verdict";
const CHECK_VERDICT: &str = "a check before queueing exited";
const AFTER_NO_VERDICT: &str = "transient";
const AFTER_VERDICTS: [&str; 2] = ["green", "red-main"];

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub at: f64,
    pub verdict: bool,
    pub run: String,
    pub line: String,
    pub refused: Option<Refused>,
}

/// The lane a check before queueing refused, the fleet session that handed it in, and that check with its first error, the lane's own name taken out so lanes refused alike compare equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub lane: String,
    pub session: String,
    pub check: String,
}

/// Every outcome under the state directory `state`, oldest first.
pub fn gather(state: &Path) -> Vec<Outcome> {
    let mut outcomes: Vec<Outcome> = fs::read_to_string(state.join(REFUSALS)).unwrap_or_default().lines().filter_map(hand_in).collect();
    outcomes.extend(ledger_rows(state).iter().filter_map(|row| after_landing(row)));
    outcomes.sort_by(|earlier, later| earlier.at.total_cmp(&later.at));
    outcomes
}

fn ledger_rows(state: &Path) -> Vec<String> {
    let queues = fs::read_dir(state.join(LAND_QUEUES)).into_iter().flatten().flatten();
    queues.flat_map(|queue| fs::read_to_string(queue.path().join(LANDER_LEDGER)).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>()).collect()
}

/// Whether a lander's after-landing run on a build host read commit `sha` green.
pub fn green_after_landing(state: &Path, repo: &str, sha: &str) -> bool {
    ledger_rows(state).iter().any(|row| {
        let columns: Vec<&str> = row.split('\t').collect();
        columns.get(1) == Some(&repo) && columns.get(3) == Some(&"green") && columns.get(2).is_some_and(|landed| landed.len() >= 7 && sha.len() >= 7 && (landed.starts_with(sha) || sha.starts_with(landed)))
    })
}

pub fn hand_in(line: &str) -> Option<Outcome> {
    let refusal: Value = serde_json::from_str(line).ok()?;
    let field = |key: &str| refusal.get(key).and_then(Value::as_str).unwrap_or_default();
    let (lane, error, first_error) = (field("lane"), field("error"), field("first_error"));
    let refused = if error.contains(NO_VERDICT) {
        None
    } else if error.starts_with(CHECK_VERDICT) {
        let check = if first_error.is_empty() { error.to_owned() } else { format!("{}: {first_error}", field("step")) };
        Some(Refused { check: if lane.is_empty() { check } else { check.replace(lane, "LANE") }, lane: lane.to_owned(), session: field("session").to_owned() })
    } else {
        return None;
    };
    let at = refusal.get("micros").and_then(Value::as_u64)? as f64 / 1e6;
    Some(Outcome { at, verdict: refused.is_some(), run: format!("hand-in {lane} ({})", field("session")), line: error.to_owned(), refused })
}

pub fn after_landing(row: &str) -> Option<Outcome> {
    let columns: Vec<&str> = row.split('\t').collect();
    let (at, repo, verdict) = (columns.first()?.parse().ok()?, columns.get(1)?, *columns.get(3)?);
    let verdict = match verdict {
        AFTER_NO_VERDICT => false,
        _ if AFTER_VERDICTS.contains(&verdict) => true,
        _ => return None,
    };
    Some(Outcome { at, verdict, run: format!("after-landing run of {repo} {}", columns.get(2).map_or("", |sha| sha.get(..10).unwrap_or(sha))), line: columns.get(4).copied().unwrap_or_default().to_owned(), refused: None })
}
