use crate::guard::permission_offer::proxy_report;
use crate::hook::{HookInput, Verdict};

const HANDOFF_PHRASES: [&str; 8] = ["type this", "paste this", "run this", "type the following", "paste the following", "run the following", "in my prompt", "in your terminal"];

pub const REASON: &str = "BLOCKED (command-handoff): your reply hands Tom a shell command to run. Agents run commands themselves under standing authority and never give Tom one to type. Clear a stale git lock with `north lane unlock CHECKOUT` and a stray file your own tool call left with `north lane stray PATH`; a write a guard refuses that no sanctioned verb covers is a harness gap to file as an issue and report, not a command for Tom. Run it yourself, or rewrite the reply without the command.";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let session = std::env::var(crate::fleet::runtime::SESSION_ENV).unwrap_or_default();
    decide(input, &session)
}

pub fn decide(input: &HookInput, session: &str) -> Verdict {
    match proxy_report(input, session).as_deref().is_some_and(hands_over_a_command) {
        true => Verdict::Deny(REASON.into()),
        false => Verdict::Allow,
    }
}

pub fn hands_over_a_command(text: &str) -> bool {
    let bang_line = text.lines().any(|line| {
        let rest = line.trim_start().strip_prefix('!').map(str::trim_start);
        rest.is_some_and(|rest| rest.starts_with(|first: char| first.is_ascii_lowercase() || "~/.$".contains(first)))
    });
    let lower = text.to_lowercase();
    bang_line || (text.contains("```") && HANDOFF_PHRASES.iter().any(|phrase| lower.contains(phrase)))
}

#[cfg(test)]
#[path = "command_handoff_tests.rs"]
mod tests;
