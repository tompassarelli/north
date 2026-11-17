//! Whether a lane-only session may hand in its own lane through safe-push (north#1650).

use std::process::Command;

/// The refusal a lane-only session's shell gets instead of running safe-push itself, `None` when its lane may land
/// from it: every OpenCode and Pi shell command runs in the broker's fresh `--lane` child, outside the session's own
/// scope, so the broker cannot name the caller, and such a safe-push proceeds only when `needed`, the review the
/// review-accept scan asks of the lane, is empty. A lane that changes ring-0 paths or adds a dependency needs a
/// reviewer's accept and lands from a Claude session (north#1650). It names the lane, branch and commit to hand over.
pub fn safe_push_refusal(needed: impl FnOnce() -> Result<Vec<String>, String>) -> Option<String> {
    if !north_base::platform::session_scope::lane_only_outside_scope() {
        return None;
    }
    let why = match needed() {
        Ok(needed) if needed.is_empty() => return None,
        Ok(needed) => format!("the lane changes ring-0 paths or adds dependencies ({}), so it needs a review accept and must come from a Claude session", needed.join(", ")),
        Err(error) => format!("the review-accept scan could not read the lane ({error}), so it cannot be told to need no accept"),
    };
    let fact = |arguments: &[&str]| {
        Command::new("git")
            .args(arguments)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|line| !line.is_empty())
    };
    let worktree = fact(&["rev-parse", "--show-toplevel"]).unwrap_or_else(|| ".".to_owned());
    let branch = fact(&["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_else(|| "HEAD".to_owned());
    let sha = fact(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    Some(format!("safe-push: lane-only session: {why}; end with Done: {worktree} {branch} {sha}"))
}
