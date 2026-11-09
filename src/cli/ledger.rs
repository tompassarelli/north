use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::ledger::{self, Ledger};

const USAGE: &str = "Usage: north ledger (flow|incident|tempo|worker) ... | north ledger duplicate [--days N] [--json] | north ledger runs [--json] | north ledger claude-runs --settled FILE --recorded FILE [--session ID] [--since DATE] | north task landed REF SHA | north task check REF -- CMD... | north task check --done-when [--dry-run] [--main DIR] (--body-file FILE | REPO#N | --repo OWNER/REPO)";

pub fn run(arguments: &[String]) -> ExitCode {
    let outcome = match arguments {
        [ledger, verb, rest @ ..] if ledger == "ledger" && let Some(run) = super::tools::ledger_runner(verb) => return ExitCode::from(run(rest)),
        [ledger, verb, rest @ ..] if ledger == "ledger" && verb == "claude-runs" => {
            crate::providers::claude::runs::run(rest).map(|()| ExitCode::SUCCESS)
        }
        [ledger, verb, rest @ ..] if ledger == "ledger" && verb == "duplicate" => return ExitCode::from(super::ledger_duplicate::run(rest)),
        _ => Ledger::open_default().and_then(|store| dispatch(&store, arguments)),
    };
    match outcome {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::from(error.exit_code())
        }
    }
}

fn dispatch(store: &Ledger, arguments: &[String]) -> NorthResult<ExitCode> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["ledger", "runs"] => ledger::print_runs(store, false).map(|()| ExitCode::SUCCESS),
        ["ledger", "runs", "--json"] => ledger::print_runs(store, true).map(|()| ExitCode::SUCCESS),
        ["task", "landed", task_ref, revision] => {
            let sha = ledger::landed(store, &std::env::current_dir()?, task_ref, revision)?;
            println!("{task_ref} landed {sha}");
            Ok(ExitCode::SUCCESS)
        }
        ["task", "check", rest @ ..] if rest.contains(&"--done-when") => {
            let rest: Vec<String> = rest.iter().map(|word| (*word).to_owned()).collect();
            crate::ops::done_when_boxes::run(store, &rest).map(|()| ExitCode::SUCCESS)
        }
        ["task", "check", rest @ ..] if rest.first().is_some_and(|word| ["--observations", "--dry-run", "--body-file", "--repo"].contains(word)) => {
            let rest: Vec<String> = rest.iter().map(|word| (*word).to_owned()).collect();
            crate::ops::observation_boxes::run(&rest).map(|()| ExitCode::SUCCESS)
        }
        ["task", "check", task_ref, "--", command @ ..] if !command.is_empty() => {
            let argv: Vec<String> = command.iter().map(|word| (*word).to_owned()).collect();
            let exit = ledger::check(store, task_ref, &argv)?;
            eprintln!("{task_ref} check exit {exit}");
            Ok(ExitCode::from(u8::try_from(exit).unwrap_or(1)))
        }
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}
