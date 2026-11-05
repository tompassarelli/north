use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::{agent_catalog, build_cache_gate, contract_probe, grounding, guard, health, hook, language_gate, nix_explain, runtime_floor_gate, work_items};

mod abandon_witness;
mod agents;
mod cloud;
mod cloud_handback;
mod cloud_land;
mod cloud_reap;
mod token_usage;
mod fleet;
mod gate;
mod hook_stats;
mod ledger;
mod orders;
mod probe_account;
pub(crate) mod remote_land;
mod remote_serve;
#[cfg(test)]
mod remote_serve_tests;
mod send;
mod safe_push_result;
mod session;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod laptop_runner_tests;
#[cfg(test)]
mod fleet_peer_row_tests;
#[cfg(test)]
mod remote_land_tests;
#[cfg(test)]
mod south_land_tests;
#[cfg(test)]
mod south_policy_tests;
#[cfg(test)]
mod south_test_repo;
mod tools;
mod verbs;
mod worker_ledger;

use session::Session;
use tools::Tool;

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Fleet(Vec<String>),
    Help,
    DesktopIdleState(Option<String>),
    Hook(Vec<String>),
    Nix(Vec<String>),
    Threads(Vec<String>),
    Ledger(Vec<String>),
    Agents(Vec<String>),
    Work(Vec<String>),
    Session(Session, Vec<String>),
    Tool(Tool, Vec<String>),
    LanguageGate(Vec<String>),
    BuildCacheGate(Vec<String>),
    RuntimeFloorGate(Vec<String>),
    Health(Vec<String>),
    GuardLanguage,
    ContractProbe(Vec<String>),
    Grounding(Vec<String>),
    RemoteServe(Vec<String>),
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

type Forward = fn(&str, Vec<String>) -> Command;

const FORWARDED: &[(&str, Forward)] = &[
    ("hook", |_, rest| Command::Hook(rest)),
    ("nix", |_, rest| Command::Nix(rest)),
    ("threads", |_, rest| Command::Threads(rest)),
    ("fleet", |_, rest| Command::Fleet(rest)),
    ("ledger", |name, rest| Command::Ledger([vec![name.to_owned()], rest].concat())),
    ("task", |name, rest| Command::Ledger([vec![name.to_owned()], rest].concat())),
    ("orders", |name, rest| Command::Fleet([vec![name.to_owned()], rest].concat())),
    ("gate", |name, rest| Command::Fleet([vec![name.to_owned()], rest].concat())),
    ("digest", |name, rest| Command::Fleet([vec![name.to_owned()], rest].concat())),
    ("work", |_, rest| Command::Work(rest)),
    ("language-gate", |_, rest| Command::LanguageGate(rest)),
    ("build-cache-gate", |_, rest| Command::BuildCacheGate(rest)),
    ("runtime-floor-gate", |_, rest| Command::RuntimeFloorGate(rest)),
    ("health", |_, rest| Command::Health(rest)),
    ("contract-probe", |_, rest| Command::ContractProbe(rest)),
    ("grounding", |_, rest| Command::Grounding(rest)),
    ("remote-serve", |_, rest| Command::RemoteServe(rest)),
];

fn forwarded(command: &str, rest: Vec<String>) -> Option<Command> {
    if let Some(tool) = Tool::named(command) {
        return Some(Command::Tool(tool, rest));
    }
    if let Some(session) = Session::named(command) {
        return Some(Command::Session(session, rest));
    }
    FORWARDED.iter().find(|(name, _)| *name == command).map(|(name, forward)| forward(name, rest))
}

pub fn main() -> ExitCode {
    match parse_command(env::args().skip(1)) {
        Ok(command) => dispatch(command),
        Err(error) => failure(error),
    }
}

fn hook_command(arguments: Vec<String>) -> ExitCode {
    let reported = |outcome: Result<(), String>| match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    };
    match arguments.first().map(String::as_str) {
        Some("stats") => reported(hook_stats::run(&arguments[1..])),
        Some("prompt-grounding") if arguments.iter().any(|argument| argument == "--help") => {
            println!("usage: north hook prompt-grounding   (reads one UserPromptSubmit payload on stdin)");
            ExitCode::SUCCESS
        }
        Some("prompt-grounding") => {
            grounding::prompt_hook(&|| guard::activation::authoring_guards_off("prompt-grounding"), &grounded_session);
            ExitCode::SUCCESS
        }
        _ => {
            guard::worker_handoff::install_confirm(crate::pacing::confirm_spawn);
            guard::host::install(guard::host::Host { pace: crate::pacing::answer, picked_tier: crate::dispatch::picked_tier });
            reported(hook::run(&arguments))
        }
    }
}

fn grounded_session(conversation: &str) -> bool {
    let named = std::env::var("NORTH_FLEET_SESSION").ok();
    let entrypoint = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
    crate::fleet::audience::grounded(&crate::fleet::audience::fleet_dir(), named.as_deref(), conversation, entrypoint.as_deref())
}

fn dispatch(command: Command) -> ExitCode {
    match command {
        Command::DesktopIdleState(state) => match state {
            Some(value) if guard::desktop_action::record_idle(value == "idle").is_ok() => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        },
        Command::Hook(arguments) => hook_command(arguments),
        Command::Nix(arguments) => match nix_explain::run(&arguments) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::Threads(arguments) => ExitCode::from(crate::threads::client::run(&arguments, std::sync::Arc::new(fleet::Registry))),
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
        Command::Session(session, arguments) => in_runtime(async move { session.run(&arguments).await.map(ExitCode::from) }),
        Command::Tool(tool, arguments) => tool.run(&arguments),
        Command::LanguageGate(arguments) => in_runtime(async move { Ok(language_gate::run_gate(&arguments)) }),
        Command::BuildCacheGate(arguments) => in_runtime(async move { Ok(build_cache_gate::run_gate(&arguments)) }),
        Command::RuntimeFloorGate(arguments) => in_runtime(async move { Ok(runtime_floor_gate::run_gate(&arguments)) }),
        Command::Health(arguments) => in_runtime(async move { Ok(health::run(&arguments, |repo| crate::ops::citation_check::verdict(repo, tools::defines))) }),
        Command::GuardLanguage => in_runtime(async { Ok(language_gate::run_guard()) }),
        Command::ContractProbe(arguments) => in_runtime(async move {
            contract_probe::run(&arguments, probe_account::invocation).await.map(ExitCode::from)
        }),
        Command::RemoteServe(arguments) => match remote_serve::run(&arguments) {
            Ok(code) => ExitCode::from(code),
            Err(error) => failure(error),
        },
        Command::Grounding(arguments) => in_runtime(async move {
            grounding::main(&arguments, &grounded_session).map(|code| ExitCode::from(code as u8))
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
