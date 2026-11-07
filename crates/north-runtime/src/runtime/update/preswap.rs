use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout, TREE};
use crate::platform::{host, process};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(60);
const ALARMED: &str = "preswap-held";

/// The incoming runtime reads every north call on this host (units, the scripts they run, hooks and its own ssh hops)
/// against its own command table before current moves to it, so a runtime that dropped a command a caller here still
/// runs is held while the live one stays; each distinct refusal alarms once.
pub(super) fn check(layout: &Layout, target: &str, previous: Option<&Path>, notify: &mut dyn FnMut(&str)) -> Result<String, Failure> {
    let runtime = layout.runtime(target);
    let mut command = Command::new(runtime.join("north"));
    command.args(["check", "calls", "--tree"]).arg(runtime.join(TREE));
    if let Some(live) = previous.and_then(layout::linked_sha) {
        command.arg("--live").arg(layout.runtime(live));
    }
    let (passed, said) = match process::try_run_command_within(command, None, LIMIT) {
        Ok(Some(output)) => (output.status.success(), format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))),
        Ok(None) => (false, format!("north check calls gave no verdict within {}s", LIMIT.as_secs())),
        Err(error) => (false, format!("north check calls did not start: {error}")),
    };
    let lines: Vec<&str> = said.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let short = layout::short(target);
    let record = layout.state.join(ALARMED);
    if passed {
        let _ = fs::remove_file(&record);
        return Ok(lines.iter().map(|line| format!("north runtime: {short} on {}: {line}\n", host::name())).collect());
    }
    let live = previous.and_then(layout::linked_sha).map_or("no runtime", layout::short);
    let message = format!("{short} is held before its swap on {}, which stays on {live}: {}", host::name(), lines.join(" | "));
    let seen = format!("{target} {}", lines.join("\n"));
    if fs::read_to_string(&record).ok().as_deref() != Some(seen.as_str()) {
        let _ = fs::write(&record, &seen);
        notify(&message);
    }
    Err(failed(1, message))
}
