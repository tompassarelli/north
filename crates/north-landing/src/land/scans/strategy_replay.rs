//! `north land scan strategy-replay BASE..HEAD`: a lane that touches a decision the decision log
//! records replays the last day of logged cases with its own build of north before it is pushed, and
//! is refused when a count shifts past its bound or an invariant breaks (docs/rationale/strategy-kernel.md).
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use super::{Range, git};

const DECIDES: [&str; 15] = [
    "crates/north-dispatch/src/pacing/placement/",
    "crates/north-pace/src/pace/",
    "crates/north-capacity/src/capacity/",
    "crates/north-landing/src/land/bisect.rs",
    "crates/north-landing/src/land/checks.rs",
    "crates/north-landing/src/land/candidate.rs",
    "crates/north-landing/src/land/culprit.rs",
    "crates/north-landing/src/land/serve.rs",
    "crates/north-landing/src/land/sheriff.rs",
    "crates/north-landing/src/land/steps.rs",
    "crates/north-landing/src/land/judge_case.rs",
    "crates/north-strategy-types/",
    "crates/north-replay/",
    "crates/north-base/src/platform/decision_log.rs",
    "harness/defaults.toml",
];
const HEAD_NORTH: &str = "bin/head-north";
const REPLAY: [&str; 6] = ["strategy", "replay", "--since", "24h", "--invariants", "--budget"];
const TRAILER: &str = "Replay-shift:";

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
    let _ = writeln!(out, "strategy-replay: the lane touches {}; replaying the last 24 hours of logged decisions with its build", touched.join(", "));
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
    let already = refused(&String::from_utf8_lossy(&main.stdout));
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
