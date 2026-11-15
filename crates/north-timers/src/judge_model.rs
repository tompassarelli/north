use std::path::PathBuf;
use std::time::Duration;

use crate::runtime_access::RuntimeAccess;
use crate::timers::oneshot::{self, Asked};

const ATTEMPT_LIMIT: Duration = Duration::from_secs(90);

pub struct Model<'a> {
    pub model: &'a str,
    pub effort: &'a str,
    pub system: &'a str,
}

/// Asks each judge in turn until one answers: the Claude subscription accounts, then the Claude API credit pool. No
/// judge runs a client with tools, since the prompt carries a session's text (north#1553); a judge that fails
/// only moves the ask on, and a run where none answers stays unjudged, so the push stays refused (fail closed).
pub fn ask_subscription(model: &Model, prompt: &str, access: &RuntimeAccess) -> Result<(String, String), String> {
    let question = oneshot::Ask { model: model.model, effort: model.effort, system: model.system, input: prompt };
    let answered = std::cell::RefCell::new(None);
    let subscription = || match oneshot::ask_claude_answered(&question, ATTEMPT_LIMIT, access, &mut |line| eprintln!("privacy judge: {line}")) {
        (Asked::Answer(answer), by) => {
            *answered.borrow_mut() = by;
            Ok(answer)
        }
        (Asked::AtLimit, _) => Err("every claude subscription account is at its usage limit".to_owned()),
        (Asked::Failed(reason), _) => Err(reason),
    };
    let api = || ask_api_pool(model, prompt);
    let (answer, judge) = first_answer(&[("subscription", &subscription), ("api credit", &api)])?;
    let by = answered.borrow().clone().filter(|_| judge == "subscription").map_or_else(|| judge.to_owned(), |place| format!("{judge}:{place}"));
    Ok((answer, by))
}

type Judge<'a> = (&'a str, &'a dyn Fn() -> Result<String, String>);

/// The first answer of `judges` in order, or every judge's refusal when none answers.
fn first_answer<'a>(judges: &[Judge<'a>]) -> Result<(String, &'a str), String> {
    let mut refused = Vec::new();
    for (name, ask) in judges {
        match ask() {
            Ok(answer) => return Ok((answer, name)),
            Err(why) => {
                eprintln!("privacy judge: {name}: {why}");
                refused.push(format!("{name}: {why}"));
            }
        }
    }
    Err(refused.join("; "))
}

/// Asks `model` on each api organization the pool would launch on, soonest-expiring credit first; under the owner's API hold it is refused before any key is read.
fn ask_api_pool(model: &Model, prompt: &str) -> Result<String, String> {
    if let Some(held) = crate::spend::hold::at(&crate::platform::orchestration::path()) {
        return Err(held.line("api"));
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = home.join(".local/state/north");
    let profile = std::env::var_os("NORTH_PROFILES").map_or_else(|| state.join("profiles"), PathBuf::from).join("claude-api");
    let (policy, here) = (crate::platform::orchestration::path(), crate::platform::host::name());
    let mut skipped: Vec<String> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    while let Some((id, key)) = crate::providers::claude::api_pool::next_key(&policy, &home, &state, &here, &skipped) {
        skipped.push(id);
        match ask_on_key(model, prompt, &key.org, &key.file, &profile) {
            Ok(answer) => return Ok(answer),
            Err(why) => refused.push(why),
        }
    }
    if refused.is_empty() { Err("no api organization has credit left".to_owned()) } else { Err(refused.join("; ")) }
}

fn ask_on_key(model: &Model, prompt: &str, org: &str, file: &std::path::Path, profile: &std::path::Path) -> Result<String, String> {
    let secret = match std::fs::read_to_string(file) {
        Ok(secret) if !secret.trim().is_empty() => secret.trim().to_owned(),
        _ => return Err(format!("{}: key unreadable", org)),
    };
    let mut command = std::process::Command::new(north_base::platform::private_text::claude_program());
    for name in crate::spend::billing::EXTERNAL {
        command.env_remove(name);
    }
    command.env("ANTHROPIC_API_KEY", secret).env("CLAUDE_CONFIG_DIR", profile).env("CLAUDE_CODE_DISABLE_FAST_MODE", "1").env(oneshot::QUIET, "1");
    command.args(oneshot::lean_flags(&oneshot::Ask { model: model.model, effort: model.effort, system: model.system, input: prompt }));
    match crate::platform::process::run_command_within(command, Some(prompt.as_bytes()), ATTEMPT_LIMIT) {
        Some(output) if output.status.success() && !output.stdout.is_empty() => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(output) => Err(format!("{}: exited {:?}: {}{}", org, output.status.code(), String::from_utf8_lossy(&output.stdout).trim(), String::from_utf8_lossy(&output.stderr).trim())),
        None => Err(format!("{}: did not finish", org)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_judge_that_fails_moves_the_ask_to_the_next_and_none_answering_names_every_refusal_north_1049() {
        let later = std::cell::Cell::new(false);
        let refuse = || Err::<String, String>("at its usage limit".to_owned());
        let answer = || Ok::<String, String>("NONE".to_owned());
        let after = || {
            later.set(true);
            Ok("too late".to_owned())
        };
        assert_eq!(first_answer(&[("subscription", &refuse), ("api credit", &answer), ("later", &after)]), Ok(("NONE".to_owned(), "api credit")));
        assert!(!later.get(), "a judge after the one that answered was asked");
        let none = first_answer(&[("subscription", &refuse), ("api credit", &refuse), ("later", &refuse)]).unwrap_err();
        assert!(["subscription:", "api credit:", "later:"].iter().all(|name| none.contains(name)), "{none}");
    }
}
