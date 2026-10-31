use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::fleet::session_host;
use crate::{agent_catalog, contract_probe, grounding, guard, health, hook, language_gate, work_items};

mod abandon_witness;
mod agents;
mod cloud;
mod cloud_handback;
mod token_usage;
mod fleet;
mod fleet_admission;
mod fleet_claims;
mod fleet_duplicate;
mod fleet_launch;
mod fleet_lineage;
mod fleet_list;
mod fleet_placement;
mod fleet_rebooted;
mod fleet_revive;
mod fleet_spend;
mod fleet_waits;
mod fleet_tier;
mod fleet_tree;
mod fleet_usage;
mod fleet_view;
mod gate;
mod ledger;
mod orders;
mod probe_account;
mod relay;
mod safe_push_result;
#[cfg(test)]
mod tests;
mod tools;
mod worker_ledger;

use tools::Tool;

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Fleet(Vec<String>),
    Help,
    DesktopIdleState(Option<String>),
    Hook(Vec<String>),
    Threads(Vec<String>),
    Ledger(Vec<String>),
    Agents(Vec<String>),
    Work(Vec<String>),
    SessionHost(Vec<String>),
    Tool(Tool, Vec<String>),
    LanguageGate(Vec<String>),
    Health(Vec<String>),
    GuardLanguage,
    ContractProbe(Vec<String>),
    Grounding(Vec<String>),
}

const CLI_HELP: &str = include_str!("help.txt");

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<Command> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(Command::Fleet(Vec::new()));
    }
    if arguments[0] == "desktop-idle-state" {
        return Ok(Command::DesktopIdleState(arguments.get(1).cloned()));
    }
    if let Some(command) = forwarded(&arguments[0], arguments[1..].to_vec()) {
        return Ok(command);
    }
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "-h" | "--help" | "help") {
        return Ok(Command::Help);
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "agents" {
        return Ok(Command::Agents(arguments[2..].to_vec()));
    }
    if arguments == ["guard", "language"] {
        return Ok(Command::GuardLanguage);
    }
    let kind = if arguments[0].starts_with('-') {
        "option"
    } else {
        "command"
    };
    Err(NorthError::Usage(format!(
        "unrecognized {kind} '{}'\n\nUsage: north [COMMAND]\n\nRun 'north' to open the TUI, or 'north --help' for help.",
        arguments.join(" ")
    )))
}

fn forwarded(command: &str, rest: Vec<String>) -> Option<Command> {
    if let Some(tool) = Tool::named(command) {
        return Some(Command::Tool(tool, rest));
    }
    Some(match command {
        "hook" => Command::Hook(rest),
        "threads" => Command::Threads(rest),
        "fleet" => Command::Fleet(rest),
        "ledger" | "task" => Command::Ledger([vec![command.to_owned()], rest].concat()),
        "orders" | "gate" => Command::Fleet([vec![command.to_owned()], rest].concat()),
        "work" => Command::Work(rest),
        "session-host" => Command::SessionHost(rest),
        "language-gate" => Command::LanguageGate(rest),
        "health" => Command::Health(rest),
        "contract-probe" => Command::ContractProbe(rest),
        "grounding" => Command::Grounding(rest),
        _ => return None,
    })
}

pub fn main() -> ExitCode {
    match parse_command(env::args().skip(1)) {
        Ok(command) => dispatch(command),
        Err(error) => failure(error),
    }
}

fn dispatch(command: Command) -> ExitCode {
    match command {
        Command::DesktopIdleState(state) => match state {
            Some(value) if guard::desktop_action::record_idle(value == "idle").is_ok() => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        },
        Command::Hook(arguments) => {
            guard::worker_handoff::install_confirm(crate::ops::pacing::confirm_spawn);
            match hook::run(&arguments) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::from(2)
                }
            }
        }
        Command::Threads(arguments) => ExitCode::from(crate::threads::client::run(&arguments, std::sync::Arc::new(fleet_view::Registry))),
        Command::Fleet(arguments) => fleet::run(&arguments),
        Command::Ledger(arguments) => ledger::run(&arguments),
        Command::Help => in_runtime(async {
            print!("{CLI_HELP}");
            Ok(ExitCode::SUCCESS)
        }),
        Command::Agents(arguments) => in_runtime(async move {
            agent_catalog::run(&arguments).map(|()| ExitCode::SUCCESS)
        }),
        Command::Work(arguments) => in_runtime(async move {
            work_items::run(&arguments).await.map(|()| ExitCode::SUCCESS)
        }),
        Command::SessionHost(arguments) => in_runtime(async move {
            session_host::run(&arguments, fleet::host_watch).await.map(ExitCode::from)
        }),
        Command::Tool(tool, arguments) => tool.run(&arguments),
        Command::LanguageGate(arguments) => in_runtime(async move { Ok(language_gate::run_gate(&arguments)) }),
        Command::Health(arguments) => in_runtime(async move { Ok(health::run(&arguments, tools::defines)) }),
        Command::GuardLanguage => in_runtime(async { Ok(language_gate::run_guard()) }),
        Command::ContractProbe(arguments) => in_runtime(async move {
            contract_probe::run(&arguments, probe_account::invocation).await.map(ExitCode::from)
        }),
        Command::Grounding(arguments) => in_runtime(async move {
            grounding::main(&arguments).map(|code| ExitCode::from(code as u8))
        }),
    }
}

#[tokio::main]
async fn in_runtime(work: impl Future<Output = NorthResult<ExitCode>>) -> ExitCode {
    match work.await {
        Ok(code) => code,
        Err(error) => failure(error),
    }
}

fn failure(error: NorthError) -> ExitCode {
    eprintln!("error: {}", error.user_message());
    if matches!(error, NorthError::Usage(_)) {
        ExitCode::from(2)
    } else {
        ExitCode::FAILURE
    }
}
