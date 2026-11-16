use std::sync::Arc;

use crate::agent_catalog::{self, sync_gate};
use crate::error::{NorthError, NorthResult};
use crate::fleet::runtimes::Runtimes;
use crate::pacing;
use crate::providers::claude::{self, fleet::Claude};

use super::oneshot::RuntimeAccess;
use super::{fleet, token_usage};

const USAGE: &str = "usage: north config agents {status [--json]|stale [--json]|inspect <id> [--json]|path <id>|on <id>|off <id>|sync|check-hook-paths|check-owners|live-refs}";

pub(super) fn claude_access() -> RuntimeAccess {
    RuntimeAccess { runtimes: Runtimes::new(vec![Arc::new(Claude)]), invocation: claude::invocation }
}

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

pub(super) fn usage(arguments: &[String]) -> u8 {
    if let Some(provider) = gate_provider(arguments) {
        return pace(&["gate".to_owned(), provider]);
    }
    if crate::usage::handles(arguments) {
        return with_usage_host(|host| crate::usage::run(arguments, host));
    }
    eprintln!("{USAGE}");
    2
}

pub(in crate::cli) fn with_usage_host<T>(work: impl FnOnce(&crate::usage::Host) -> T) -> T {
    let env: std::collections::BTreeMap<String, String> = std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect();
    let ssh = move |arguments: &[&str]| crate::platform::ssh::argv(&["ssh".into()], &env, arguments);
    work(&crate::usage::Host {
        name: crate::platform::host::name(),
        refresh_snapshot: &crate::threads::client::refresh_mirrored_snapshot,
        ingest: &crate::threads::client::ingest,
        run: &|command, limit| crate::platform::process::run_command_within(command, None, limit),
        ssh: &ssh,
        token_headers: &token_usage::headers,
    })
}

fn gate_provider(arguments: &[String]) -> Option<String> {
    let precedes = arguments.iter().any(|argument| argument == "--write-pace-gate");
    let at = arguments.iter().position(|argument| argument == "--gate")?;
    arguments.get(at + 1).filter(|_| !precedes).cloned()
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

fn pace(arguments: &[String]) -> u8 {
    pacing::run(
        arguments,
        pacing::FleetFacts {
            roster: fleet::roster,
            machine_load: fleet::fleet_load,
            workers: crate::dispatch::fleet_workers,
        },
        token_usage::read,
    )
}

pub(super) fn config(arguments: &[String]) -> std::process::ExitCode {
    super::in_runtime(async move { Ok(std::process::ExitCode::from(run(arguments))) })
}
