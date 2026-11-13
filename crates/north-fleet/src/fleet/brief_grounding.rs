//! The full grounding query at launch: every fresh worker launch whose brief asks for a code change
//! appends the brief's `north graph ground query` form to its first prompt, read as of the lane's base
//! commit, so the worker starts from what the graph already knows instead of finding it in its first
//! tool calls. The query is bounded, and a graph that is missing or slow leaves a line saying so in
//! place of the pack, never a failed launch.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_base::platform::process::try_run_command_within;

use super::brief_files;
use super::model::Machine;

/// The line a brief carries when the launch put the pack in it; `north ledger grounding` counts this.
pub const HEADING: &str = "Grounding (auto-retrieved, full query form, as of the lane's base commit";
const DEADLINE: Duration = Duration::from_millis(1500);
const EMPTY: &str = "- the graph named nothing on this brief";

/// The pack for `brief`, headed by the lane's base commit; `None` when the brief asks for no code change
/// or `cwd` holds no repository with a base to read.
pub fn section(brief: &str, cwd: &Path, machine: Option<&Machine>, north: &Path) -> Option<String> {
    if !north_pace::router::brief::code_change(brief) {
        return None;
    }
    let (base, seconds) = base(cwd, machine)?;
    let mut command = Command::new(north);
    command.args(["graph", "ground", "query", "--query", brief]).env("NORTH_GROUNDING_AS_OF", seconds.to_string()).env_remove("NORTH_GROUNDING_PRIOR");
    let output = try_run_command_within(command, None, DEADLINE).ok().flatten()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let named = text.trim();
    let body = if output.status.success() && !named.is_empty() {
        named.to_owned()
    } else if output.status.success() {
        EMPTY.to_owned()
    } else {
        format!("- the query failed: {}", String::from_utf8_lossy(&output.stderr).trim())
    };
    Some(format!("{HEADING} {base}):\n{body}"))
}

/// The lane's base commit and its time, read where the lane runs.
fn base(cwd: &Path, machine: Option<&Machine>) -> Option<(String, u64)> {
    let listed = brief_files::run(machine, cwd, &format!("{} && git show -s --format='%H %ct' \"$base\"", brief_files::BASE))?;
    let text = String::from_utf8_lossy(&listed);
    let line = text.lines().last()?;
    let mut words = line.split_whitespace();
    let sha = words.next()?;
    let seconds = words.next()?.parse().ok()?;
    Some((sha.to_owned(), seconds))
}
