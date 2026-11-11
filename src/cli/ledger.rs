use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::ledger::{self, Ledger};

const USAGE: &str = "Usage: north ledger (flow|incident|opencode|tempo|worker) ... | north ledger duplicate [--days N] [--json] | north ledger runs [--json] | north ledger claude-runs --settled FILE --recorded FILE [--session ID] [--since DATE] | north task landed REF SHA | north task comment REPO#N --body TEXT | north task check REF -- CMD... | north task check --done-when [--dry-run] [--main DIR] (--body-file FILE | REPO#N | --repo OWNER/REPO) | north task rewrite-cargo [--dry-run] [--repo OWNER/REPO] [--tree DIR]";

pub fn run(arguments: &[String]) -> ExitCode {
    let outcome = match arguments {
        [task, verb, ..] if task == "task" && (verb == "comment" || verb == "check" && arguments.iter().any(|word| word == "--done-when")) && let Some(code) = north_runtime::worker_sandbox::reenter_lane_only(arguments) => return ExitCode::from(code),
        [task, verb, reference, flag, body] if task == "task" && verb == "comment" && flag == "--body" => comment(reference, body),
        [ledger, verb, rest @ ..] if ledger == "ledger" && let Some(run) = super::tools::ledger_runner(verb) => return ExitCode::from(run(rest)),
        [ledger, verb, rest @ ..] if ledger == "ledger" && verb == "claude-runs" => {
            crate::providers::claude::runs::run(rest).map(|()| ExitCode::SUCCESS)
        }
        [ledger, verb, rest @ ..] if ledger == "ledger" && verb == "duplicate" => return ExitCode::from(super::ledger_duplicate::run(rest)),
        [task, verb, rest @ ..] if task == "task" && verb == "rewrite-cargo" => crate::ops::cargo_boxes::run(rest).map(|()| ExitCode::SUCCESS),
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

fn comment(reference: &str, body: &str) -> NorthResult<ExitCode> {
    let (repository, number) = reference.split_once('#').and_then(|(repository, number)| Some((repository, number.parse::<u64>().ok()?))).filter(|(repository, _)| !repository.is_empty()).ok_or_else(|| NorthError::Usage(format!("{reference} is not REPO#N")))?;
    let repository = if repository.contains('/') { repository.to_owned() } else { format!("tompassarelli/{repository}") };
    crate::platform::github::comment(&repository, number, body).map_err(NorthError::Usage)?;
    println!("commented on {repository}#{number}");
    Ok(ExitCode::SUCCESS)
}
