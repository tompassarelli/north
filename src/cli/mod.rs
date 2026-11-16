use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::{agent_catalog, build_cache_gate, contract_probe, grounding, guard, language_gate, nix_explain};

mod abandon_witness;
mod mutant_issues;
mod agents;
mod call_check;
mod cloud;
mod cloud_handback;
pub(crate) mod concern_history;
mod cloud_land;
mod cloud_reap;
mod token_usage;
mod alarm_stats;
mod box_path;
mod fetch_fleet;
mod fleet_for;
mod fleet_issue;
mod proof_run;
mod fleet;
mod steps;
mod gate;
mod gate_closing;
mod gate_list;
mod gate_relay;
mod glance;
mod holds;
mod hook_verb;
mod ledger;
mod ledger_duplicate;
mod ledger_show;
mod machines;
mod moved;
mod nix;
mod operator;
mod orders;
mod probe_account;
pub(crate) mod remote_land;
mod draft_road;
mod land_adopt;
mod control_plane;
mod remote_serve;
#[cfg(test)]
mod remote_serve_tests;
mod dash;
mod infra;
pub(crate) mod scoreboard;
mod send;
mod session;
mod spend;
#[cfg(test)]
mod usage_error_tests;
#[cfg(test)]
mod laptop_runner_tests;
#[cfg(test)]
mod repeat_unblock_tests;
#[cfg(test)]
mod fleet_peer_row_tests;
#[cfg(test)]
mod south_land_tests;
#[cfg(test)]
mod south_policy_tests;
#[cfg(test)]
mod south_test_repo;
mod tools;
mod verb_help;
mod verbs;
mod views;
mod work_items;
mod worker_ledger;

use session::Session;
use tools::Tool;

pub(crate) use fleet::coordinator_chain::validate as validate_coordinator_chain;

/// The `north threads` verb families that live above north-threads, run beside the ones the ledger runs itself.
pub(crate) const THREADS_FAMILIES: &[crate::threads::verb::Family] = &[crate::threads::calibration::FAMILY];

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Fleet(Vec<String>),
    Help,
    HelpAll,
    DesktopIdleState(Option<String>),
    Alarm(Vec<String>),
    Hook(Vec<String>),
    Nix(Vec<String>),
    NixGc(Vec<String>),
    Threads(Vec<String>),
    Ledger(Vec<String>),
    Agents(Vec<String>),
    ConfigCheck(Vec<String>),
    Work(Vec<String>),
    Accounts(Vec<String>),
    Session(Session, Vec<String>),
    Tool(Tool, Vec<String>),
    LanguageGate(Vec<String>),
    BuildCacheGate(Vec<String>),
    Health(Vec<String>),
    Glance(Vec<String>),
    Holds(Vec<String>),
    Operator(Vec<String>),
    BoxPath(Vec<String>),
    GuardLanguage,
    ContractProbe(Vec<String>),
    Grounding(Vec<String>),
    RemoteServe(Vec<String>),
    VerbHelp(Vec<String>),
    Switch(Vec<String>),
}

const CLI_HELP: &str = include_str!("help.txt");
const CLI_REFERENCE: &str = include_str!("reference.txt");

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<Command> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(Command::Fleet(Vec::new()));
    }
    if answers_help(&arguments) {
        return Ok(Command::VerbHelp(verb_help::path(&arguments)));
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
    if arguments == ["--help-all"] {
        return Ok(Command::HelpAll);
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "agents" {
        return Ok(Command::Agents(arguments[2..].to_vec()));
    }
    if arguments.len() >= 3 && arguments[0] == "config" && matches!([arguments[1].as_str(), arguments[2].as_str()], ["context", "audit"] | ["coordinator", "calls" | "role"]) {
        return Ok(Command::Tool(Tool::Config, arguments[1..].to_vec()));
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "check" {
        return Ok(Command::ConfigCheck(arguments[2..].to_vec()));
    }
    if arguments == ["guard", "language"] {
        return Ok(Command::GuardLanguage);
    }
    Err(moved::refusal(&arguments))
}

type Forward = fn(&str, Vec<String>) -> Command;

const FORWARDED: &[(&str, Forward)] = &[
    ("alarm", |_, rest| Command::Alarm(rest)),
    ("hook", |_, rest| Command::Hook(rest)),
    ("nix", |_, rest| match rest.split_first() {
        Some((verb, tail)) if verb == "gc" => Command::NixGc(tail.to_vec()),
        _ => Command::Nix(rest),
    }),
    ("threads", |_, rest| Command::Threads(rest)),
    ("fleet", |name, rest| match rest.split_first() {
        Some((verb, _)) if verb == "off" || verb == "on" => Command::Switch([vec![name.to_owned()], rest].concat()),
        Some((verb, tail)) if let Some(tool) = Tool::fleet_verb(verb) => Command::Tool(tool, tail.to_vec()),
        _ => Command::Fleet(rest),
    }),
    ("ledger", |name, rest| Command::Ledger([vec![name.to_owned()], rest].concat())),
    ("task", |name, rest| Command::Ledger([vec![name.to_owned()], rest].concat())),
    ("gate", |name, rest| Command::Fleet([vec![name.to_owned()], rest].concat())),
    ("work", |_, rest| match rest.first().map(String::as_str) {
        Some("brief" | "review" | "split") => Command::Tool(Tool::Work, rest),
        _ => Command::Work(rest),
    }),
    ("accounts", |_, rest| Command::Accounts(rest)),
    ("language-gate", |_, rest| Command::LanguageGate(rest)),
    ("build-cache-gate", |_, rest| Command::BuildCacheGate(rest)),
    ("health", |_, rest| Command::Health(rest)),
    ("glance", |_, rest| Command::Glance(rest)),
    ("holds", |_, rest| Command::Holds(rest)),
    ("box", |_, rest| Command::BoxPath(rest)),
    ("contract-probe", |_, rest| Command::ContractProbe(rest)),
    ("remote-serve", |_, rest| Command::RemoteServe(rest)),
    ("policy", |name, rest| Command::Switch([vec![name.to_owned()], rest].concat())),
];

fn forwarded(command: &str, rest: Vec<String>) -> Option<Command> {
    if command == "graph" && rest.first().is_some_and(|verb| verb == "ground") {
        return Some(Command::Grounding(rest[1..].to_vec()));
    }
    if command == "config" && rest.first().is_some_and(|verb| verb == "operator") {
        return Some(Command::Operator(rest[1..].to_vec()));
    }
    if let Some(tool) = Tool::named(command) {
        return Some(Command::Tool(tool, rest));
    }
    if let Some(session) = Session::named(command) {
        return Some(Command::Session(session, rest));
    }
    FORWARDED.iter().find(|(name, _)| *name == command).map(|(name, forward)| forward(name, rest))
}

pub fn answers_help(arguments: &[String]) -> bool {
    arguments.first().is_some_and(|verb| tools::defines(verb)) && verb_help::requested(arguments)
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
        Command::Alarm(arguments) => match alarm_stats::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::Hook(arguments) => hook_verb::run(arguments),
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
        Command::NixGc(arguments) => nix::gc(&arguments),
        Command::Threads(arguments) => ExitCode::from(ledger::threads(&arguments)),
        Command::Fleet(arguments) => fleet::run(&arguments),
        Command::Ledger(arguments) => ledger::run(&arguments),
        Command::VerbHelp(path) => match verb_help::usage(&path) {
            Some(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            None => failure(NorthError::Usage(format!("north {} has no line in north --help-all", path.join(" ")))),
        },
        Command::Help => in_runtime(async {
            print!("{CLI_HELP}");
            Ok(ExitCode::SUCCESS)
        }),
        Command::HelpAll => {
            print!("{CLI_REFERENCE}");
            ExitCode::SUCCESS
        }
        Command::Agents(arguments) => agents::config(&arguments),
        Command::ConfigCheck(arguments) => in_runtime(async move { agent_catalog::config_check(&arguments).map(ExitCode::from) }),
        Command::Work(arguments) => match arguments.first().map(String::as_str) {
            None => views::work::run(&arguments).into(),
            Some("own") => views::work::own(&arguments[1..]).into(),
            Some(flag) if flag.starts_with('-') => views::work::run(&arguments).into(),
            Some(_) => in_runtime(async move { work_items::run(&arguments).await.map(|()| ExitCode::SUCCESS) }),
        },
        Command::Accounts(arguments) => views::accounts::run(&arguments).into(),
        Command::Session(session, arguments) => in_runtime(async move { session.run(&arguments).await.map(ExitCode::from) }),
        Command::Tool(tool, arguments) => tool.run(&arguments),
        Command::LanguageGate(arguments) => in_runtime(async move { Ok(language_gate::run_gate(&arguments)) }),
        Command::BuildCacheGate(arguments) => in_runtime(async move { Ok(build_cache_gate::run_gate(&arguments)) }),
        Command::Health(arguments) => views::health::run(&arguments, |repo| crate::citation_check::verdict(repo, tools::defines)),
        Command::Glance(arguments) => match glance::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error),
        },
        Command::Holds(arguments) => match holds::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error),
        },
        Command::Operator(arguments) => match operator::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error),
        },
        Command::BoxPath(arguments) => match box_path::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error),
        },
        Command::GuardLanguage => in_runtime(async { Ok(language_gate::run_guard()) }),
        Command::ContractProbe(arguments) => in_runtime(async move {
            contract_probe::run(&arguments, probe_account::invocation).await.map(ExitCode::from)
        }),
        Command::RemoteServe(arguments) => match remote_serve::run(&arguments) {
            Ok(code) => ExitCode::from(code),
            Err(error) => failure(error),
        },
        Command::Grounding(arguments) => in_runtime(async move {
            grounding::main(&arguments).map(|code| ExitCode::from(code as u8))
        }),
        Command::Switch(arguments) => switch(&arguments),
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
    ExitCode::from(error.exit_code())
}

fn switch(arguments: &[String]) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let error = std::process::Command::new("north-switch").args(arguments).exec();
    failure(NorthError::Configuration(format!("north {}: north-switch, which the host configuration installs, does not start: {error}", arguments.join(" "))))
}
