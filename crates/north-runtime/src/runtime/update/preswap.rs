use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout, TREE};
use crate::platform::{host, process};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(60);
const ALARMED: &str = "preswap-held";
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
    let checked = run(layout, target, live.map(|live| layout.runtime(live)).as_deref());
    let short = layout::short(target);
    let report = checked.lines.iter().map(|line| format!("north runtime: {short} on {}: {line}\n", host::name())).collect::<String>();
    let record = layout.state.join(ALARMED);
    if checked.passed {
        let _ = fs::remove_file(&record);
        return Ok(report);
    }
    let before = live.filter(|live| *live != target).map(|live| run(layout, live, None)).filter(|before| !before.passed);
    let introduced: Vec<&str> = match &before {
        Some(before) if checked.refusals().next().is_some() => checked.refusals().filter(|line| !before.refusals().any(|seen| seen == *line)).map(String::as_str).collect(),
        _ => checked.lines.iter().map(String::as_str).collect(),
    };
    let live = live.map_or("no runtime", layout::short);
    let mut page = |seen: String, message: &str| {
        if fs::read_to_string(&record).ok().as_deref() != Some(seen.as_str()) {
            let _ = fs::write(&record, &seen);
            notify(message);
        }
    };
    if introduced.is_empty() {
        let refusals: Vec<&str> = checked.refusals().map(String::as_str).collect();
        let message = format!("host incident on {}, not {short}'s change: {live} refuses the same calls, so {short} is promoted: {}", host::name(), refusals.join(" | "));
        page(format!("incident {}", refusals.join("\n")), &message);
        return Ok(format!("{report}north runtime: {message}\n"));
    }
    let message = format!("{short} is held before its swap on {}, which stays on {live}: {}", host::name(), introduced.join(" | "));
    page(format!("{target} {}", introduced.join("\n")), &message);
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
