//! Whose doing a finished run's outcome was: kept, the worker's (wrong approach, missed the box, ran out
//! of budget, reworked) or the harness's (a host that cannot do the box, a landing refusal, capacity,
//! compaction parking, a decision or a wait). Only worker-caused outcomes speak to a tier's fitness.
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Cause {
    Kept,
    Worker(&'static str),
    Harness(&'static str),
}

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*([a-z]+)").expect("stop pattern"));

/// What a report says held it up when it names no `Stop:` line, first match wins.
const HARNESS: [(&str, &[&str]); 7] = [
    ("decision", &["decision from tom", "needs a decision", "your decision", "tom's decision", "halted by tom", "answer from tom", "gate "]),
    ("host", &["laptop", "gpu", "can't reach", "cannot reach", "no route to", "is offline", "unreachable", "native capture", "client pair"]),
    ("landing", &["lander", "landing queue", "landing run", "landing freeze", "queued behind", "safe-push", "not on origin/main yet", "refused the landing", "landing was refused"]),
    ("capacity", &["capacity", "usage limit", "rate limit", "free slot", "out of slots"]),
    ("collision", &["another session", "another lane", "same lane", "peer session"]),
    ("compaction", &["compaction", "compacted", "context limit"]),
    ("wait", &["wait until", "can't be measured yet", "cannot be measured yet", "longer wait", "measurement over time", "review day", "-day window"]),
];

/// The cause of a run's outcome, or None when it says nothing about the worker or the harness: a split,
/// which re-scopes the box, or an unclear end with no reason a rule can read.
pub fn cause(outcome: &str, succeeded: bool, report: &str) -> Option<Cause> {
    if outcome == "split" {
        return None;
    }
    if succeeded {
        return Some(Cause::Kept);
    }
    if let Some(stopped) = STOP.captures(report).and_then(|found| stopped(&found[1].to_lowercase())) {
        return Some(stopped);
    }
    let lowered = report.to_lowercase();
    if let Some((kind, _)) = HARNESS.iter().find(|(_, phrases)| phrases.iter().any(|phrase| lowered.contains(phrase))) {
        return Some(Cause::Harness(kind));
    }
    match outcome {
        "done" => Some(Cause::Worker("reworked")),
        "unclear" | "handoff" => None,
        _ => Some(Cause::Worker("missed")),
    }
}

/// The cause a report's own `Stop:` word names.
fn stopped(word: &str) -> Option<Cause> {
    match word {
        "reasoning" => Some(Cause::Worker("reasoning")),
        "leash" => Some(Cause::Worker("leash")),
        "landing" => Some(Cause::Harness("landing")),
        "capacity" => Some(Cause::Harness("capacity")),
        "client" | "host" => Some(Cause::Harness("host")),
        "wait" => Some(Cause::Harness("wait")),
        "decision" => Some(Cause::Harness("decision")),
        _ => None,
    }
}

