//! Whether hand-ins and after-landing runs still reach verdicts: a hand-in whose checks before queueing reached no
//! verdict, or an after-landing run the lander called transient, with no verdict from either since for two minutes,
//! means no build host answers, and the page names the newest such run's own line.
use super::{Alert, Detail, Severity, WatchContext};

pub use north_landing_gates::landing::pipe_verdicts::{Outcome, gather};

const NO_VERDICT_SECONDS: f64 = 120.0;
const RECENT: f64 = 3600.0;
const CAUSE_CHARS: usize = 160;
pub(super) const LOST: &str = "north land: no verdict ";

pub(super) fn probe(context: &WatchContext) -> Vec<Alert> {
    unanswered(&context.outcomes, context.now as f64).into_iter().collect()
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
