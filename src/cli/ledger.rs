use std::path::PathBuf;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::ledger::{self, Ledger};

const USAGE: &str = "Usage: north ledger runs [--json] | north ledger recommend --planner FACETS | north ledger planner-band | north ledger summary [--json] [--since 24h] | north ledger claude-runs --settled FILE --recorded FILE [--session ID] [--since DATE] | north task landed REF SHA | north task check REF -- CMD...";

pub fn run(arguments: &[String]) -> ExitCode {
    let outcome = match arguments {
        [ledger, verb, rest @ ..] if ledger == "ledger" && verb == "claude-runs" => {
            crate::providers::claude::runs::run(rest).map(|()| ExitCode::SUCCESS)
        }
        [ledger, verb, rest @ ..] if ledger == "ledger" && verb == "summary" && rest.iter().any(|word| word == "--since") => closed_summary(rest),
        [ledger, verb, planner, facets] if ledger == "ledger" && verb == "recommend" && planner == "--planner" => {
            print_threads_report(|database| ledger::planner::recommend(database, facets))
        }
        [ledger, verb] if ledger == "ledger" && verb == "planner-band" => print_threads_report(ledger::planner::band_readout),
        _ => Ledger::open_default().and_then(|store| dispatch(&store, arguments)),
    };
    match outcome {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::FAILURE
        }
    }
}

fn dispatch(store: &Ledger, arguments: &[String]) -> NorthResult<ExitCode> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["ledger", "runs"] => ledger::print_runs(store, false).map(|()| ExitCode::SUCCESS),
        ["ledger", "runs", "--json"] => ledger::print_runs(store, true).map(|()| ExitCode::SUCCESS),
        ["ledger", "summary"] => ledger::print_summary(store, false).map(|()| ExitCode::SUCCESS),
        ["ledger", "summary", "--json"] => ledger::print_summary(store, true).map(|()| ExitCode::SUCCESS),
        ["task", "landed", task_ref, revision] => {
            let sha = ledger::landed(store, &std::env::current_dir()?, task_ref, revision)?;
            println!("{task_ref} landed {sha}");
            Ok(ExitCode::SUCCESS)
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

fn closed_summary(words: &[String]) -> NorthResult<ExitCode> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (window, json) = match words.as_slice() {
        ["--since", window] => (*window, false),
        ["--since", window, "--json"] | ["--json", "--since", window] => (*window, true),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let report = ledger::closed_report(window)?;
    let (linked, landed) = Ledger::open_default()?.landing_links(ledger::window_start_ms(window)?)?;
    if json {
        let mut figures = report.json(window);
        figures["landed_tasks"] = serde_json::json!({"landed": landed, "linked": linked});
        println!("{figures}");
    } else {
        println!("{}", report.dashboard(window));
        report.print_table();
        println!("landed tasks with a linked run: {linked} of {landed}");
    }
    Ok(ExitCode::SUCCESS)
}

fn print_threads_report(report: impl FnOnce(&std::path::Path) -> NorthResult<String>) -> NorthResult<ExitCode> {
    let database = match std::env::var_os("THREADS_DB").filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var_os("HOME").ok_or_else(|| NorthError::Configuration("HOME is not set".into()))?).join(".local/state/threads/threads.db"),
    };
    print!("{}", report(&database)?);
    Ok(ExitCode::SUCCESS)
}
