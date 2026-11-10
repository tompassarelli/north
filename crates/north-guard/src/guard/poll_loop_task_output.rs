use std::sync::LazyLock;

use regex::Regex;

use crate::hook::{HookInput, Verdict};
use crate::shell;

static TASK_OUTPUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/tmp/claude-[^\s'\x22;|&)]*/tasks/[^\s'\x22;|&)]*\.output").unwrap());
static LOOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s;&|('\x22])(?:until|while|for)\s").unwrap());
static SLEEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s;&|('\x22])sleep\s").unwrap());

pub fn verdict(input: &HookInput) -> Verdict {
    if input.runs_in_background() {
        return Verdict::Allow;
    }
    match input.command().and_then(|command| command.as_str()) {
        Some(command) if polls_task_output(command) => Verdict::Deny(refusal()),
        _ => Verdict::Allow,
    }
}

pub(super) fn polls_task_output(command: &str) -> bool {
    let text = shell::strip_heredocs(command);
    TASK_OUTPUT.is_match(&text) && LOOP.is_match(&text) && SLEEP.is_match(&text)
}

fn refusal() -> String {
    "No. This call waits in a sleep or poll loop on a background task's output file, and the harness already wakes you when that task ends. \
     End your turn and let its completion notification arrive; to watch a log meanwhile, use Monitor with an until-loop."
        .to_owned()
}
