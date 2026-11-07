//! `north land scan strategy-replay BASE..HEAD`: a lane that touches a decision the decision log
//! records replays the last day of logged cases with its own build of north before it is pushed, and
//! is refused when a count shifts past its bound or an invariant breaks (docs/rationale/strategy-kernel.md).
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use super::{Range, git};

mod lane_build;

const DECIDES: [&str; 15] = [
    "crates/north-pacing/src/pacing/placement/",
    "crates/north-capacity/src/capacity/",
    "crates/north-landing/src/land/bisect.rs",
    "crates/north-landing/src/land/checks.rs",
    "crates/north-landing/src/land/candidate.rs",
    "crates/north-landing/src/land/culprit.rs",
    "crates/north-landing/src/land/serve.rs",
    "crates/north-landing/src/land/sheriff.rs",
    "crates/north-landing/src/land/steps.rs",
    "crates/north-landing/src/land/judge_case.rs",
    "crates/north-strategy/",
    "crates/north-strategy-types/",
    "crates/north-replay/",
    "crates/north-base/src/platform/decision_log.rs",
    "harness/defaults.toml",
];
const HEAD_NORTH: &str = "bin/head-north";
const REPLAY: [&str; 7] = ["ops", "strategy", "replay", "--since", "24h", "--invariants", "--budget"];
const TRAILER: &str = "Replay-shift:";
const BUDGET_KEY: &str = "the replay took";
const SLOWER_THAN_MAIN: f64 = 1.5;

/// The paths of `DECIDES` the range changes, or none for a repository without the replay.
fn touched(directory: &Path, range: &Range) -> Vec<&'static str> {
    if !directory.join("crates/north-replay").is_dir() {
        return Vec::new();
    }
    let changed = git::text(directory, &["diff", "--name-only", &format!("{}..{}", range.base, range.head)]).unwrap_or_default();
    DECIDES.into_iter().filter(|decide| changed.lines().any(|path| path.starts_with(decide))).collect()
}

pub fn scan(directory: &Path, range: &str, out: &mut impl std::io::Write) -> u8 {
    let Some(range) = Range::parse(range) else {
        let _ = writeln!(out, "usage: north land scan strategy-replay BASE..HEAD");
        return 1;
    };
    let touched = touched(directory, &range);
    if touched.is_empty() {
        return 0;
    }
    if super::strategy_purity::scan(directory, range.head, out) != 0 {
        return 1;
    }
    let _ = writeln!(out, "strategy-replay: the lane touches {}; replaying the last 24 hours of logged decisions with its build", touched.join(", "));
    lane_build::prefetch(directory, &range, out);
    let lane = match Command::new(directory.join(HEAD_NORTH)).current_dir(directory).args(["--code", "crates/"]).args(REPLAY).stderr(Stdio::inherit()).output() {
        Ok(output) => output,
        Err(error) => {
            let _ = writeln!(out, "strategy-replay: cannot run {HEAD_NORTH}: {error}");
            return 1;
        }
    };
    let report = String::from_utf8_lossy(&lane.stdout);
    let _ = out.write_all(report.as_bytes());
    if lane.status.success() {
        return 0;
    }
    let lane_refused = refused(&report);
    let main = std::env::current_exe().and_then(|north| Command::new(north).current_dir(directory).args(REPLAY).stderr(Stdio::null()).output());
    let main = main.ok().filter(|main| lane.status.code() == Some(1) && !lane_refused.is_empty() && main.status.code().is_some_and(|code| code <= 1));
    let Some(main) = main else {
        let _ = writeln!(out, "strategy-replay: refused the landing; the replay above names the count or invariant it broke");
        return lane.status.code().map_or(1, |code| u8::try_from(code).unwrap_or(1));
    };
    let main_report = String::from_utf8_lossy(&main.stdout);
    let already = refused(&main_report);
    let mut lane_refused = lane_refused;
    if lane_refused.contains(BUDGET_KEY) && !slower_than_main(&report, &main_report) {
        lane_refused.remove(BUDGET_KEY);
        let _ = writeln!(out, "strategy-replay: the lane's replay is over its time budget but not slower than main's replay of the same build profile on this host; the budget is not held against the lane");
    }
    for key in lane_refused.clone() {
        let Some(name) = measure_name(&key) else { continue };
        if !figure_slower(&report, &main_report, name) {
            lane_refused.remove(&key);
            let _ = writeln!(out, "strategy-replay: the lane's {name} is over its bound but not past 1.5 times main's of the same build profile on this host; it is not held against the lane");
        }
    }
    let bodies = git::text(directory, &["log", "--format=%B", &format!("{}..{}", range.base, range.head)]).unwrap_or_default();
    let declared = declared(&bodies);
    let (intended, new): (Vec<String>, Vec<String>) = beyond(&lane_refused, &already).into_iter().partition(|key| declared.contains_key(key));
    for key in &intended {
        let _ = writeln!(out, "strategy-replay: {key} moves as the lane declares ({TRAILER} {key}: {})", declared[key]);
    }
    if new.is_empty() {
        let _ = writeln!(out, "strategy-replay: beyond what main's own build refuses ({}) the lane breaks nothing it does not declare; passed", already.into_iter().collect::<Vec<_>>().join(", "));
        return 0;
    }
    let _ = writeln!(out, "strategy-replay: refused the landing; the lane's build breaks {}, which main's build does not", new.join(", "));
    1
}

fn refused(report: &str) -> BTreeSet<String> {
    report
        .lines()
        .filter_map(|line| line.strip_prefix("refused: "))
        .map(|refusal| refusal.split_once(':').map_or_else(|| refusal.split(|character: char| character.is_ascii_digit()).next().unwrap_or(refusal).trim_end().to_owned(), |(key, _)| key.to_owned()))
        .collect()
}

/// The seconds and build profile a replay report ends with, on its `replayed N cases in X s (PROFILE build)` line.
fn timing(report: &str) -> Option<(f64, &str)> {
    let line = report.lines().find(|line| line.starts_with("replayed "))?;
    let (_, rest) = line.split_once(" cases in ")?;
    let (seconds, rest) = rest.split_once(" s (")?;
    Some((seconds.parse().ok()?, rest.strip_suffix(" build)")?))
}

/// Whether the lane's replay is slower than main's with the same build profile; a lane and a main built differently cannot be compared, so the lane is not named.
fn slower_than_main(lane: &str, main: &str) -> bool {
    match (timing(lane), timing(main)) {
        (Some((lane_seconds, lane_profile)), Some((main_seconds, main_profile))) => lane_profile == main_profile && lane_seconds > main_seconds * SLOWER_THAN_MAIN,
        _ => false,
    }
}

/// The figure a refusal's key is judged on in the replay's `measured:` line: a kind's slowest call, or the replay's memory growth.
fn measure_name(key: &str) -> Option<&str> {
    key.strip_suffix(" call took").or_else(|| (key == "the replay grew").then_some("growth"))
}

/// The figure the replay's `measured:` line gives for `name`, whose entries read `NAME FIGURE UNIT`.
fn figure(report: &str, name: &str) -> Option<f64> {
    let line = report.lines().find_map(|line| line.strip_prefix("measured: "))?;
    line.split(", ").find_map(|entry| entry.strip_prefix(name)?.split_whitespace().next()?.parse().ok())
}

/// Whether the lane's figure is past 1.5 times main's of the same build profile; one built differently cannot be compared, so the lane is not named.
fn figure_slower(lane: &str, main: &str, name: &str) -> bool {
    match (timing(lane), timing(main), figure(lane, name), figure(main, name)) {
        (Some((_, lane_profile)), Some((_, main_profile)), Some(lane_figure), Some(main_figure)) => lane_profile == main_profile && lane_figure > main_figure * SLOWER_THAN_MAIN,
        _ => false,
    }
}

fn beyond(lane: &BTreeSet<String>, main: &BTreeSet<String>) -> Vec<String> {
    lane.difference(main).cloned().collect()
}

/// Each `Replay-shift: KEY: REASON` line of the range's commit bodies, KEY being a refusal's key such as `placement cloud`.
fn declared(bodies: &str) -> BTreeMap<String, String> {
    bodies
        .lines()
        .filter_map(|line| line.trim().strip_prefix(TRAILER)?.split_once(':'))
        .map(|(key, reason)| (key.trim().to_owned(), reason.trim().to_owned()))
        .filter(|(key, reason)| !key.is_empty() && !reason.is_empty())
        .collect()
}

#[cfg(test)]
mod tests;
