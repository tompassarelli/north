use std::time::Duration;

use super::session::{self as oneshot, Asked};

const ATTEMPT_LIMIT: Duration = Duration::from_secs(90);

pub struct Model<'a> {
    pub model: &'a str,
    pub effort: &'a str,
    pub system: &'a str,
}

/// Asks the Claude subscription accounts in turn. No judge runs a client with tools, since the prompt carries a session's text (north#1553); a run where no account answers stays unjudged, so the push stays refused (fail closed).
pub fn ask_subscription(model: &Model, prompt: &str) -> Result<(String, String), String> {
    let question = oneshot::Ask { model: model.model, effort: model.effort, system: model.system, input: prompt };
    match oneshot::ask_claude_answered(&question, ATTEMPT_LIMIT, &mut |line| eprintln!("disclosure judge: {line}")) {
        (Asked::Answer(answer), by) => Ok((answer, by.map_or_else(|| "subscription".to_owned(), |place| format!("subscription:{place}")))),
        (Asked::AtLimit, _) => Err("every claude subscription account is at its usage limit".to_owned()),
        (Asked::Failed(reason), _) => Err(reason),
    }
}
