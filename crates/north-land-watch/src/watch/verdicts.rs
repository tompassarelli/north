//! Whether hand-ins and after-landing runs still reach verdicts: a hand-in whose checks before queueing reached no
//! verdict, or an after-landing run the lander called transient, with no verdict from either since for two minutes,
//! means no build host answers, and the page names the newest such run's own line.
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::{Alert, Detail, Severity, WatchContext};

const REFUSALS: &str = "north/enqueue-refusals.jsonl";
const LAND_QUEUES: &str = "north/land-queue";
const LANDER_LEDGER: &str = "lander-post-land.tsv";
const NO_VERDICT: &str = "reached no verdict";
const CHECK_VERDICT: &str = "a check before queueing exited";
const AFTER_NO_VERDICT: &str = "transient";
const AFTER_VERDICTS: [&str; 2] = ["green", "red-main"];
const NO_VERDICT_SECONDS: f64 = 120.0;
const RECENT: f64 = 3600.0;
const CAUSE_CHARS: usize = 160;
pub(super) const LOST: &str = "north land: no verdict ";

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub at: f64,
    pub verdict: bool,
    pub run: String,
    pub line: String,
}

#[derive(Deserialize)]
struct Refusal {
    micros: u64,
    #[serde(default)]
    lane: String,
    #[serde(default)]
    session: String,
    #[serde(default)]
    error: String,
}

pub(super) fn probe(context: &WatchContext) -> Vec<Alert> {
    unanswered(&context.outcomes, context.now as f64).into_iter().collect()
}

pub fn gather(state: &Path) -> Vec<Outcome> {
    let mut outcomes: Vec<Outcome> = fs::read_to_string(state.join(REFUSALS)).unwrap_or_default().lines().filter_map(hand_in).collect();
    let queues = fs::read_dir(state.join(LAND_QUEUES)).into_iter().flatten().flatten();
    outcomes.extend(queues.flat_map(|queue| fs::read_to_string(queue.path().join(LANDER_LEDGER)).unwrap_or_default().lines().filter_map(after_landing).collect::<Vec<_>>()));
    outcomes.sort_by(|earlier, later| earlier.at.total_cmp(&later.at));
    outcomes
}

pub(super) fn hand_in(line: &str) -> Option<Outcome> {
    let refusal: Refusal = serde_json::from_str(line).ok()?;
    let verdict = if refusal.error.contains(NO_VERDICT) {
        false
    } else if refusal.error.starts_with(CHECK_VERDICT) {
        true
    } else {
        return None;
    };
    Some(Outcome { at: refusal.micros as f64 / 1e6, verdict, run: format!("hand-in {} ({})", refusal.lane, refusal.session), line: refusal.error })
}

pub(super) fn after_landing(row: &str) -> Option<Outcome> {
    let columns: Vec<&str> = row.split('\t').collect();
    let (at, repo, verdict) = (columns.first()?.parse().ok()?, columns.get(1)?, *columns.get(3)?);
    let verdict = match verdict {
        AFTER_NO_VERDICT => false,
        _ if AFTER_VERDICTS.contains(&verdict) => true,
        _ => return None,
    };
    Some(Outcome { at, verdict, run: format!("after-landing run of {repo} {}", columns.get(2).map_or("", |sha| sha.get(..10).unwrap_or(sha))), line: columns.get(4).copied().unwrap_or_default().to_owned() })
}

pub(super) fn unanswered(outcomes: &[Outcome], now: f64) -> Option<Alert> {
    let answered = outcomes.iter().rposition(|outcome| outcome.verdict).map_or(0, |at| at + 1);
    let lost = &outcomes[answered..];
    let (first, newest) = (lost.first()?, lost.last()?);
    if now - first.at < NO_VERDICT_SECONDS || now - newest.at > RECENT {
        return None;
    }
    let cause: String = newest.line.chars().take(CAUSE_CHARS).collect();
    let summary = format!("{LOST}for {:.0} min from any hand-in or after-landing run, {} of them without one; newest: {}: {cause}", (now - first.at) / 60.0, lost.len(), newest.run);
    Some(Alert { key: "land:no-verdict".into(), severity: Severity::Page, repo: None, owner: None, summary, since: first.at as u64, detail: Detail::default() })
}
