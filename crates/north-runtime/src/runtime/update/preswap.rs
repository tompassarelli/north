use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout, TREE};
use crate::platform::{host, process};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(60);
const ALARMED: &str = "preswap-held";
const INCIDENT: &str = "preswap-host-incident";
const REFUSED: &str = "north check calls: ✗ ";

struct Checked {
    passed: bool,
    lines: Vec<String>,
}

impl Checked {
    fn refusals(&self) -> impl Iterator<Item = &String> {
        self.lines.iter().filter(|line| line.starts_with(REFUSED))
    }
}

/// A refusal the runtime this host already runs gives as well is the host's fault, not the new runtime's: it is paged once as a host incident and the swap goes ahead, so a stale unit never pins a host to an old runtime.
pub(super) fn check(layout: &Layout, target: &str, previous: Option<&Path>, notify: &mut dyn FnMut(&str)) -> Result<String, Failure> {
    let live = previous.and_then(layout::linked_sha);
    let runtime = layout.runtime(target);
    let checked = run(layout, target, live.map(|live| layout.runtime(live)).as_deref());
    let short = layout::short(target);
    let report = |lines: &[String]| lines.iter().map(|line| format!("north runtime: {short} on {}: {line}\n", host::name())).collect::<String>();
    let held = layout.state.join(ALARMED);
    let incident = layout.state.join(INCIDENT);
    if checked.passed {
        let _ = fs::remove_file(&held);
        let _ = fs::remove_file(&incident);
        return Ok(report(&checked.lines));
    }
    let shared = live.filter(|live| layout.runtime(live) != runtime).map(|live| run(layout, live, None)).filter(|before| !before.passed);
    let introduced: Vec<&String> = match &shared {
        Some(before) if checked.refusals().next().is_some() => checked.refusals().filter(|line| !before.refusals().any(|seen| seen == *line)).collect(),
        _ => checked.lines.iter().collect(),
    };
    let live = live.map_or("no runtime", layout::short);
    if introduced.is_empty() {
        let _ = fs::remove_file(&held);
        let refusals: Vec<&str> = checked.refusals().map(String::as_str).collect();
        let message = format!("host incident on {}, not {short}'s change: {live} refuses the same calls, so {short} is promoted: {}", host::name(), refusals.join(" | "));
        if fs::read_to_string(&incident).ok().as_deref() != Some(refusals.join("\n").as_str()) {
            let _ = fs::write(&incident, refusals.join("\n"));
            notify(&message);
        }
        return Ok(format!("{}north runtime: {message}\n", report(&checked.lines)));
    }
    let lines: Vec<&str> = introduced.iter().map(|line| line.as_str()).collect();
    let message = format!("{short} is held before its swap on {}, which stays on {live}: {}", host::name(), lines.join(" | "));
    let seen = format!("{target} {}", lines.join("\n"));
    if fs::read_to_string(&held).ok().as_deref() != Some(seen.as_str()) {
        let _ = fs::write(&held, &seen);
        notify(&message);
    }
    Err(failed(1, message))
}

fn run(layout: &Layout, sha: &str, live: Option<&Path>) -> Checked {
    let runtime = layout.runtime(sha);
    let mut command = Command::new(runtime.join("north"));
    command.args(["check", "calls", "--tree"]).arg(runtime.join(TREE));
    if let Some(live) = live {
        command.arg("--live").arg(live);
    }
    let (passed, said) = match process::try_run_command_within(command, None, LIMIT) {
        Ok(Some(output)) => (output.status.success(), format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))),
        Ok(None) => (false, format!("north check calls gave no verdict within {}s", LIMIT.as_secs())),
        Err(error) => (false, format!("north check calls did not start: {error}")),
    };
    let own = runtime.display().to_string();
    let lines = said.lines().map(str::trim).filter(|line| !line.is_empty()).map(|line| line.replace(&own, "RUNTIME")).collect();
    Checked { passed, lines }
}
