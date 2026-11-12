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
            guard::host::install(guard::host::Host { pace: crate::pacing::answer, picked_tier: crate::dispatch::picked_tier, recorded_decisions: grounding::bearing::records, account_home: fleet::account_home, read_streak, status_verb, file_tool_refusal: north_runtime::worker_sandbox::unwritable_by_file_tool });
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

pub(super) fn grounded_session(conversation: &str) -> bool {
    let named = std::env::var("NORTH_FLEET_SESSION").ok();
    let entrypoint = std::env::var("CLAUDE_CODE_ENTRYPOINT").ok();
    crate::fleet::audience::grounded(&crate::fleet::audience::fleet_dir(), named.as_deref(), conversation, entrypoint.as_deref())
}

fn status_verb(words: &[&str]) -> bool {
    super::verbs::VERB_KINDS.kind(words) == Some(crate::proxy::Kind::Status)
}

fn read_streak(transcript: &str, hook: &serde_json::Value, free: guard::host::FreeCall) -> guard::host::ReadStreak {
    let text = |key: &str| hook.get(key).and_then(serde_json::Value::as_str).unwrap_or_default();
    let input = hook.get("tool_input").unwrap_or(&serde_json::Value::Null);
    let call = crate::proxy::Call { id: text("tool_use_id"), tool: text("tool_name"), input };
    let streak = crate::proxy::read_streaks(transcript, &super::verbs::VERB_KINDS, Some(call), &free).pop().unwrap_or_default();
    let message = guard::host::MessageReads { looks: streak.message.looks, looking: crate::proxy::is_look(text("tool_name"), input, &super::verbs::VERB_KINDS) && !free(text("tool_name"), input) };
    guard::host::ReadStreak { calls: streak.calls(), shapes: streak.top_shapes(3), subject: streak.subject, message }
}

#[cfg(test)]
#[path = "hook_verb_tests.rs"]
mod tests;
