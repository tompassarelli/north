use std::process::ExitCode;

use super::{fleet, hook_stats};
use crate::{foundation, grounding, guard, hook};

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
        Some("stats") => reported(hook_stats::run(&arguments[1..])),
        Some("prompt-grounding") => {
            grounding::prompt_hook(|| guard::activation::authoring_guards_off("prompt-grounding"), grounded_session);
            ExitCode::SUCCESS
        }
        _ => {
            guard::host::install(guard::host::Host { pace: crate::pacing::answer, picked_tier: crate::dispatch::picked_tier, recorded_decisions: grounding::bearing::records, account_home: fleet::account_home });
            let started = std::time::Instant::now();
            match foundation::hook_deadline::within(started + GUARD_DEADLINE, move || hook::run(&arguments)) {
                Some(outcome) => reported(outcome),
                None => {
                    foundation::hook_deadline::record_error(&foundation::hook_deadline::errors_file(), "north-hook", "deadline");
                    ExitCode::SUCCESS
                }
            }
        }
    }
}

pub(super) fn grounded_session(conversation: &str) -> bool {
    let named = std::env::var("NORTH_FLEET_SESSION").ok();
    let entrypoint = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
    crate::fleet::audience::grounded(&crate::fleet::audience::fleet_dir(), named.as_deref(), conversation, entrypoint.as_deref())
}
