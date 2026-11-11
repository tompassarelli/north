use std::path::Path;
use std::process::{Command, Stdio};

use super::after::follow;

/// The free then total land checks the reachable build hosts take at once, read from `north capacity offload slots`: each host's free cores over the cores one check uses, capped at the checks it keeps for the lander.
pub fn free_slots(repo: &Path) -> Option<(usize, usize)> {
    let output = Command::new(follow::program()).args(["capacity", "offload", "slots"]).current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut words = text.split_whitespace().map(str::parse::<usize>);
    Some((words.next()?.ok()?, words.next()?.ok()?)).filter(|(free, total)| free <= total)
}
