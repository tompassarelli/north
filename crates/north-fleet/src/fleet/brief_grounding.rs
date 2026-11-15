//! The full grounding query at launch: every fresh worker launch whose brief asks for a code change
//! appends the brief's `north graph ground query` form to its first prompt, read as of the lane's base
//! commit, so the worker starts from what the graph already knows instead of finding it in its first
//! tool calls. The query is bounded by the time a warm client takes to come up, and a graph that is
//! missing or slow leaves a line naming the query in place of the pack, never a failed launch
//! (north#1531).
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use north_base::platform::process::try_run_command_within;

use super::brief_files;
use super::model::Machine;

/// The line a brief carries when the launch put the pack in it; `north ledger grounding` counts this.
pub const HEADING: &str = "Grounding (auto-retrieved, full query form, as of the lane's base commit";
/// The graph's share of a warm-spare launch: a pack that misses it leaves the query's line in the
/// prompt instead, so the client's first user line waits for the client and not for a slow graph.
/// A guess from the 627-1670 ms client reads and the 1.8-2.2 s the full query took on gex45; the
/// launch's own `north fleet hops` reading settles it (north#1531).
const DEADLINE: Duration = Duration::from_millis(500);
const EMPTY: &str = "- the graph named nothing on this brief";
const SLOW: &str = "- the graph did not answer inside the launch's budget; run `north graph ground query` on this brief before your first edit";

/// The pack for `brief`, headed by the lane's base commit; `None` when the brief asks for no code change
/// or `cwd` holds no repository with a base to read.
pub fn section(brief: &str, cwd: &Path, machine: Option<&Machine>, north: &Path) -> Option<String> {
    if !north_pace::router::brief::code_change(brief) {
        return None;
    }
    let until = Instant::now() + DEADLINE;
    let (base, seconds) = base(cwd, machine, left(until))?;
    let mut command = Command::new(north);
    command.args(["graph", "ground", "query", "--query", brief]).env("NORTH_GROUNDING_AS_OF", seconds.to_string()).env_remove("NORTH_GROUNDING_PRIOR");
    let body = match try_run_command_within(command, None, left(until)).ok().flatten() {
        None => SLOW.to_owned(),
        Some(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let named = text.trim();
            if output.status.success() && !named.is_empty() {
                named.to_owned()
            } else if output.status.success() {
                EMPTY.to_owned()
            } else {
                format!("- the query failed: {}", String::from_utf8_lossy(&output.stderr).trim())
            }
        }
    };
    Some(format!("{HEADING} {base}):\n{body}"))
}

/// What is left of the section's budget, never zero so a call still has a moment to answer.
fn left(until: Instant) -> Duration {
    until.saturating_duration_since(Instant::now()).max(Duration::from_millis(1))
}

/// The lane's base commit and its time, read where the lane runs.
fn base(cwd: &Path, machine: Option<&Machine>, limit: Duration) -> Option<(String, u64)> {
    let listed = brief_files::run_within(machine, cwd, &format!("{} && git show -s --format='%H %ct' \"$base\"", brief_files::BASE), limit)?;
    let text = String::from_utf8_lossy(&listed);
    let line = text.lines().last()?;
    let mut words = line.split_whitespace();
    let sha = words.next()?;
    let seconds = words.next()?.parse().ok()?;
    Some((sha.to_owned(), seconds))
}
