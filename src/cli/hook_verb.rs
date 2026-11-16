use std::process::ExitCode;

use super::fleet;
use crate::{foundation, guard, hook};

const GUARD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(8);

pub(super) fn run(arguments: Vec<String>) -> ExitCode {
    let reported = |outcome: Result<(), String>| match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    };
    match arguments.first().map(String::as_str) {
        Some("language-guard") => {
            if !guard_off("language-guard") {
                let _ = crate::language_gate::run_guard();
            }
            ExitCode::SUCCESS
        }
        Some("firn-system-policy") => {
            guard::firn_system_policy::run(&arguments[1..]);
            ExitCode::SUCCESS
        }
        first => {
            if let Some(unit) = dispatcher_unit(first)
                && guard_off(unit)
            {
                return ExitCode::SUCCESS;
            }
            guard::host::install(guard::host::Host { account_home: fleet::account_home, status_verb, file_tool_refusal: north_runtime::worker_sandbox::unwritable_by_file_tool, file_read_refusal: north_runtime::worker_sandbox::unreadable_by_file_tool });
            let started = std::time::Instant::now();
            let mut request = Vec::new();
            let _ = std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut request);
            let (missed, verb) = (request.clone(), arguments.clone());
            let deadline = std::env::var("NORTH_HOOK_DEADLINE_MS").ok().and_then(|ms| ms.parse().ok()).map_or(GUARD_DEADLINE, std::time::Duration::from_millis);
            let worked = move || {
                let mut answer = Vec::new();
                let outcome = hook::run(&arguments, &request, &mut answer).map(drop);
                (outcome, answer)
            };
            match foundation::hook_deadline::within(started + deadline, worked) {
                Some((outcome, answer)) => {
                    let _ = std::io::Write::write_all(&mut std::io::stdout().lock(), &answer);
                    reported(outcome)
                }
                None => {
                    foundation::hook_deadline::record_error(&foundation::hook_deadline::errors_file(), "north-hook", "deadline");
                    if let Some(answer) = hook::deadline_answer(&verb, &missed) {
                        println!("{answer}");
                    }
                    ExitCode::SUCCESS
                }
            }
        }
    }
}

fn dispatcher_unit(verb: Option<&str>) -> Option<&'static str> {
    match verb {
        None | Some("dispatch") => Some("north-hook"),
        Some("codex-conduct") => Some("codex-conduct"),
        Some(_) => None,
    }
}

fn guard_off(unit: &str) -> bool {
    let off = guard::activation::authoring_guards_off(unit);
    if off {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
    }
    off
}

fn status_verb(words: &[&str]) -> bool {
    super::verbs::VERB_KINDS.kind(words) == Some(crate::coordinator::Kind::Status)
}
