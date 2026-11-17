use crate::agent_catalog::{self, sync_gate};
use crate::error::{NorthError, NorthResult};

const USAGE: &str = "usage: north config agents {status [--json]|stale [--json]|inspect <id> [--json]|path <id>|on <id>|off <id>|sync|check-hook-paths|check-owners|live-refs}";

pub(super) fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        None | Some("status" | "stale" | "inspect" | "path" | "on" | "off" | "check-hook-paths" | "check-owners" | "live-refs") => exit(agent_catalog::run(arguments).map(|()| 0)),
        Some("sync") => exit(sync_gate::run(arguments)),
        Some(_) => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn exit(outcome: NorthResult<u8>) -> u8 {
    outcome.unwrap_or_else(|error| {
        eprintln!("error: {}", error.user_message());
        if matches!(error, NorthError::Usage(_)) {
            2
        } else {
            1
        }
    })
}

pub(super) fn config(arguments: &[String]) -> std::process::ExitCode {
    super::in_runtime(async move { Ok(std::process::ExitCode::from(run(arguments))) })
}
