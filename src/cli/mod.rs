use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::{agent_catalog, build_cache_gate, guard, language_gate, nix_explain};

mod abandon_witness;
mod mutant_issues;
mod agents;
mod call_check;
mod cloud;
mod job_run;
mod cloud_handback;
pub(crate) mod concern_history;
mod cloud_land;
mod proof_run;
mod holds;
mod hook_verb;
mod task;
mod moved;
mod operator;
pub(crate) mod remote_land;
mod draft_road;
mod remote_serve;
#[cfg(test)]
mod remote_serve_tests;
mod infra;
pub(crate) mod oneshot;
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

use tools::Tool;

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    HelpAll,
    DesktopIdleState(Option<String>),
    Hook(Vec<String>),
    Nix(Vec<String>),
    Task(Vec<String>),
    Agents(Vec<String>),
    ConfigCheck(Vec<String>),
    Tool(Tool, Vec<String>),
    LanguageGate(Vec<String>),
    BuildCacheGate(Vec<String>),
    Holds(Vec<String>),
    Operator(Vec<String>),
    GuardLanguage,
    RemoteServe(Vec<String>),
    VerbHelp(Vec<String>),
    Switch(Vec<String>),
}

const CLI_HELP: &str = include_str!("help.txt");
const CLI_REFERENCE: &str = include_str!("reference.txt");

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<Command> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(Command::Help);
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
    ("hook", |_, rest| Command::Hook(rest)),
    ("nix", |_, rest| Command::Nix(rest)),
    ("task", |name, rest| Command::Task([vec![name.to_owned()], rest].concat())),
    ("work", |_, rest| Command::Tool(Tool::Work, rest)),
    ("language-gate", |_, rest| Command::LanguageGate(rest)),
    ("build-cache-gate", |_, rest| Command::BuildCacheGate(rest)),
    ("holds", |_, rest| Command::Holds(rest)),
    ("remote-serve", |_, rest| Command::RemoteServe(rest)),
    ("policy", |name, rest| Command::Switch([vec![name.to_owned()], rest].concat())),
];

fn forwarded(command: &str, rest: Vec<String>) -> Option<Command> {
    if command == "fleet" && rest.first().is_some_and(|verb| verb == "off" || verb == "on") {
        return Some(Command::Switch([vec![command.to_owned()], rest].concat()));
    }
    if command == "config" && rest.first().is_some_and(|verb| verb == "operator") {
        return Some(Command::Operator(rest[1..].to_vec()));
    }
    if let Some(tool) = Tool::named(command) {
        return Some(Command::Tool(tool, rest));
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
        Command::Task(arguments) => task::run(&arguments),
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
        Command::Tool(tool, arguments) => tool.run(&arguments),
        Command::LanguageGate(arguments) => in_runtime(async move { Ok(language_gate::run_gate(&arguments)) }),
        Command::BuildCacheGate(arguments) => in_runtime(async move { Ok(build_cache_gate::run_gate(&arguments)) }),
        Command::Holds(arguments) => match holds::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error),
        },
        Command::Operator(arguments) => match operator::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error),
        },
        Command::GuardLanguage => in_runtime(async { Ok(language_gate::run_guard()) }),
        Command::RemoteServe(arguments) => match remote_serve::run(&arguments) {
            Ok(code) => ExitCode::from(code),
            Err(error) => failure(error),
        },
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
