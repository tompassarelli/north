//! `north land scan strategy-replay BASE..HEAD`: a lane that touches a decision the decision log
//! records replays the last day of logged cases with its own build of north before it is pushed, and
//! is refused when a count shifts past its bound or an invariant breaks (docs/rationale/strategy-kernel.md).
use std::path::Path;
use std::process::Command;

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
    let status = Command::new(directory.join(HEAD_NORTH))
        .current_dir(directory)
        .args(["--code", "crates/", "strategy", "replay", "--since", "24h", "--invariants", "--budget"])
        .status();
    match status {
        Ok(status) if status.success() => 0,
        Ok(status) => {
            let _ = writeln!(out, "strategy-replay: refused the landing; the replay above names the count or invariant it broke");
            status.code().map_or(1, |code| u8::try_from(code).unwrap_or(1))
        }
        Err(error) => {
            let _ = writeln!(out, "strategy-replay: cannot run {HEAD_NORTH}: {error}");
            1
        }
    }
}
