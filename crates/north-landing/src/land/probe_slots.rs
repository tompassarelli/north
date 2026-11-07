use std::path::Path;
use std::process::{Command, Stdio};

use super::after::follow;

pub const MOST_AT_ONCE: usize = 6;

/// The free then total land checks the reachable build hosts take at once, read from `north capacity offload slots`: each host's free cores over the cores one check uses.
pub fn free_slots(repo: &Path) -> Option<(usize, usize)> {
    let output = Command::new(follow::program()).args(["capacity", "offload", "slots"]).current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut words = text.split_whitespace().map(str::parse::<usize>);
    Some((words.next()?.ok()?, words.next()?.ok()?)).filter(|(free, total)| free <= total)
}

/// The probes one round may run side by side, and the free and total offload slots it read.
pub fn slots(repo: &Path) -> (usize, Option<(usize, usize)>) {
    let read = free_slots(repo);
    (read.map_or(MOST_AT_ONCE, |(free, _)| free.max(1)), read)
}
