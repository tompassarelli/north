use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::{
    agent_catalog, autoland_status, contract_probe, dep_gate, fleet, follows_release, grounding, guard,
    health, hook, issue_box_lint, language_gate, provenance, session_host, threads_client, tui,
    work_items,
};

#[cfg(test)]
mod tests;

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Tui,
    Resume(String),
    Help,
    DesktopIdleState(Option<String>),
    Hook(Vec<String>),
    Threads(Vec<String>),
    Fleet(Vec<String>),
    Agents(Vec<String>),
    Work(Vec<String>),
    SessionHost(Vec<String>),
    DepGate(Vec<String>),
    AutolandStatus(Vec<String>),
    LanguageGate(Vec<String>),
    Health(Vec<String>),
    GuardLanguage,
    ContractProbe(Vec<String>),
    Provenance(Vec<String>),
    IssueBoxLint(Vec<String>),
    FollowsRelease(Vec<String>),
    Grounding(Vec<String>),
}

const CLI_HELP: &str = "North — interactive coding workspace

Usage: north [COMMAND]

Run north with no arguments to open the TUI.

Commands:
  health [--dry-run]                             Check harness invariants
  language-gate --repo DIR --base REV --head REV  Check language policy
  guard language                                PreToolUse language guard
  grounding [refresh|query|hook|distill|score]    Retrieve existing infrastructure and history
  work [list|search|show|claim|release|need|unneed|close]  Work items
  config agents [sync|status|on|off|path|inspect]  Manage skills and hooks
  session-host --name N --generation G --state-dir D -- ARGV  Host one headless runtime
  dep-gate BASE HEAD                            Check dependency changes before landing
  autoland-status --sha SHA --branch BRANCH      Read this lane's Autoland result
  contract-probe --validate FIXTURE | --runtime PATH  Check the claude stream contract
  provenance [check|debt|audit]                  Audit grounding and target debt
  issue-box-lint [REPO#N...] [--repo REPO]       Check issue acceptance boxes
  threads ARGV                                  Run a NorthThreads command on the ledger host
  follows-release [--repo-dir DIR]             Start queued boxes after predecessors land
  help                                          Show this help

Options:
  --resume ID  Open this conversation
  -h, --help  Show this help";

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<Command> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(Command::Tui);
    }
    match arguments[0].as_str() {
        "desktop-idle-state" => return Ok(Command::DesktopIdleState(arguments.get(1).cloned())),
        "hook" => return Ok(Command::Hook(arguments[1..].to_vec())),
        "threads" => return Ok(Command::Threads(arguments[1..].to_vec())),
        "fleet" => return Ok(Command::Fleet(arguments[1..].to_vec())),
        _ => {}
    }
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "-h" | "--help" | "help") {
        return Ok(Command::Help);
    }
    if arguments.first().is_some_and(|argument| argument == "--resume") {
        if arguments.len() == 2 && !arguments[1].is_empty() && !arguments[1].starts_with('-') {
            return Ok(Command::Resume(arguments[1].clone()));
        }
        return Err(NorthError::Usage("Usage: north --resume ID".into()));
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "agents" {
        return Ok(Command::Agents(arguments[2..].to_vec()));
    }
    if arguments[0] == "work" {
        return Ok(Command::Work(arguments[1..].to_vec()));
    }
    if arguments[0] == "session-host" {
        return Ok(Command::SessionHost(arguments[1..].to_vec()));
    }
    if arguments[0] == "dep-gate" {
        return Ok(Command::DepGate(arguments[1..].to_vec()));
    }
    if arguments[0] == "autoland-status" {
        return Ok(Command::AutolandStatus(arguments[1..].to_vec()));
    }
    if arguments[0] == "language-gate" {
        return Ok(Command::LanguageGate(arguments[1..].to_vec()));
    }
    if arguments[0] == "health" {
        return Ok(Command::Health(arguments[1..].to_vec()));
    }
    if arguments == ["guard", "language"] {
        return Ok(Command::GuardLanguage);
    }
    if arguments[0] == "contract-probe" {
        return Ok(Command::ContractProbe(arguments[1..].to_vec()));
    }
    if arguments[0] == "provenance" {
        return Ok(Command::Provenance(arguments[1..].to_vec()));
    }
    if arguments[0] == "issue-box-lint" {
        return Ok(Command::IssueBoxLint(arguments[1..].to_vec()));
    }
    if arguments[0] == "follows-release" {
        return Ok(Command::FollowsRelease(arguments[1..].to_vec()));
    }
    if arguments[0] == "grounding" {
        return Ok(Command::Grounding(arguments[1..].to_vec()));
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

pub(crate) fn main() -> ExitCode {
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
        Command::Hook(arguments) => match hook::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::Threads(arguments) => ExitCode::from(threads_client::run(&arguments)),
        Command::Fleet(arguments) => fleet::run(&arguments),
        Command::Help => in_runtime(async {
            println!("{CLI_HELP}");
            Ok(ExitCode::SUCCESS)
        }),
        Command::Agents(arguments) => in_runtime(async move {
            agent_catalog::run(&arguments).map(|()| ExitCode::SUCCESS)
        }),
        Command::Work(arguments) => in_runtime(async move {
            work_items::run(&arguments).await.map(|()| ExitCode::SUCCESS)
        }),
        Command::SessionHost(arguments) => in_runtime(async move {
            session_host::run(&arguments).await.map(ExitCode::from)
        }),
        Command::DepGate(arguments) => in_runtime(async move { Ok(ExitCode::from(dep_gate::run(&arguments))) }),
        Command::AutolandStatus(arguments) => in_runtime(async move { Ok(ExitCode::from(autoland_status::run(&arguments))) }),
        Command::LanguageGate(arguments) => in_runtime(async move { Ok(language_gate::run_gate(&arguments)) }),
        Command::Health(arguments) => in_runtime(async move { Ok(health::run(&arguments)) }),
        Command::GuardLanguage => in_runtime(async { Ok(language_gate::run_guard()) }),
        Command::ContractProbe(arguments) => in_runtime(async move {
            contract_probe::run(&arguments).await.map(ExitCode::from)
        }),
        Command::Provenance(arguments) => in_runtime(async move { Ok(ExitCode::from(provenance::run(&arguments))) }),
        Command::IssueBoxLint(arguments) => in_runtime(async move { Ok(ExitCode::from(issue_box_lint::run(&arguments))) }),
        Command::FollowsRelease(arguments) => in_runtime(async move { Ok(ExitCode::from(follows_release::run(&arguments))) }),
        Command::Grounding(arguments) => in_runtime(async move {
            grounding::main(&arguments).map(|code| ExitCode::from(code as u8))
        }),
        Command::Tui => in_runtime(open_tui(None)),
        Command::Resume(conversation) => in_runtime(open_tui(Some(conversation))),
    }
}

async fn open_tui(requested_conversation: Option<String>) -> NorthResult<ExitCode> {
    tui::run(env::current_dir()?, requested_conversation).await?;
    Ok(ExitCode::SUCCESS)
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
