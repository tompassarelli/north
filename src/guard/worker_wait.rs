use crate::hook::{HookInput, Verdict};
use crate::pyjson::Value;
use crate::shell;
use regex::Regex;
use std::sync::LazyLock;

const LIMIT_SECONDS: f64 = 60.0;
const COMMAND_START: &str = r"(?:^|[;&|(\n`])\s*(?:(?:[A-Za-z_]\w*=\S*|timeout\s+[\d.]+[smhd]?|env|nice|nohup|exec)\s+)*";

struct Wait {
    label: &'static str,
    pattern: Regex,
    inert: bool,
}

fn wait(label: &'static str, pattern: &str, inert: bool) -> Wait {
    let pattern = if inert {
        format!("{COMMAND_START}{pattern}")
    } else {
        pattern.to_owned()
    };
    Wait {
        label,
        pattern: Regex::new(&pattern).unwrap(),
        inert,
    }
}

static WAITS: LazyLock<Vec<Wait>> = LazyLock::new(|| {
    vec![
        wait("an until/sleep wait loop", r"\buntil\b[\s\S]*\bsleep\b", false),
        wait("a while/sleep wait loop", r"\bwhile\b[\s\S]*\bsleep\b", false),
        wait("safe-push", r"(?:\S*/)?safe-push\b", true),
        wait(
            "a CI watch",
            r"(?:gh\s+run\s+watch\b|gh\s+pr\s+checks\b[^\n]*--watch)",
            true,
        ),
        wait(
            "a leased run",
            r"(?:bun\s+(?:\S*/)?|\S*/)?machine-capacity\.mjs\s+(?:run|session)\b",
            true,
        ),
        wait("a farm run", r"bun\s+wisp\s+farm\b", true),
    ]
});
static TEST_SUITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bbun\s+(?:run\s+)?test\b").unwrap());
static SINGLE_TEST_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s+\S+\.test\.ts").unwrap());
static LONG_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:sleep|timeout)\s+(\d+)").unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    match refusal(input) {
        Some(reason) if !crate::hook::authoring_guards_off("worker-wait-guard") => {
            Verdict::Deny(reason)
        }
        _ => Verdict::Allow,
    }
}

pub(super) fn refusal(input: &HookInput) -> Option<String> {
    let is_worker = input.data["agent_type"]
        .as_str()
        .is_some_and(|kind| kind.starts_with("worker"));
    if input.data["tool_name"] != "Bash"
        || !is_worker
        || input.tool_input["run_in_background"] == Value::Bool(true)
    {
        return None;
    }
    let reason = blocking(
        input.command()?.as_str()?,
        timeout_seconds(&input.tool_input["timeout"]),
    )?;
    Some(format!(
        "Workers never block in the foreground on {reason}: the lead's messages reach you only between tool calls and must arrive at once. Rerun it with run_in_background: true and wait for its completion notification or a Monitor event, checking messages in between."
    ))
}

fn timeout_seconds(timeout: &Value) -> Option<f64> {
    match timeout {
        Value::Number { raw, .. } => raw.parse::<f64>().ok().map(|ms| ms / 1000.0),
        _ => None,
    }
}

pub(super) fn blocking(command: &str, timeout_seconds: Option<f64>) -> Option<String> {
    if let Some(seconds) = timeout_seconds.filter(|s| *s > LIMIT_SECONDS) {
        return Some(format!("a timeout of {} s", seconds.floor() as u64));
    }
    let inert = shell::blank_quotes(&shell::strip_heredocs(command));
    for wait in WAITS.iter() {
        let text = if wait.inert { &inert } else { command };
        if wait.pattern.is_match(text) {
            return Some(wait.label.to_owned());
        }
    }
    let runs_suite = TEST_SUITE
        .find_iter(command)
        .any(|found| !SINGLE_TEST_FILE.is_match(&command[found.end()..]));
    if runs_suite {
        return Some("a test suite".to_owned());
    }
    LONG_COMMAND.captures_iter(command).find_map(|found| {
        let over = found[1].parse::<u64>().map_or(true, |n| n as f64 > LIMIT_SECONDS);
        over.then(|| format!("`{}`", &found[0]))
    })
}
