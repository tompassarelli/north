use crate::hook_input::{HookInput, Verdict};
use crate::platform::pyjson::Value;
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
        wait("safe-push", r"(?:[^\s=]*/)?safe-push(?:[ \t]+(\S+)|[;&|)\s]|$)", true),
        wait(
            "a CI watch",
            r"(?:gh\s+run\s+watch\b|gh\s+pr\s+checks\b[^\n]*--watch)",
            true,
        ),
        wait(
            "a leased run",
            r"(?:(?:bun\s+(?:\S*/)?|\S*/)?machine-capacity\.mjs|(?:\S*/)?north\s+capacity\s+host)\s+(?:run|session)\b",
            true,
        ),
        wait("a farm run", r"bun\s+wisp\s+farm\b", true),
    ]
});
static TEST_SUITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"{COMMAND_START}bun\s+(?:run\s+)?test\b")).unwrap());
static SHORT_BOUND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\btimeout\s+(\d+)s?\s+(?:ba)?sh\s+-c\s").unwrap());
const SAFE_PUSH_QUERIES: [&str; 3] = ["--help", "-h", "--durable-harness-paths"];
static SINGLE_TEST_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s+\S+\.test\.ts").unwrap());
static LONG_TIMEOUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\btimeout\s+(\d+)").unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    match refusal(input) {
        Some(reason) => Verdict::Deny(reason),
        _ => Verdict::Allow,
    }
}

pub(super) fn refusal(input: &HookInput) -> Option<String> {
    let is_worker = input.data["agent_type"]
        .as_str()
        .is_some_and(|kind| kind.starts_with("worker"));
    if input.data["tool_name"] != "Bash"
        || !is_worker
        || input.runs_in_background()
    {
        return None;
    }
    if is_hand_in(input.command()?.as_str()?) {
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

/// Whether the command is the one hand-in call alone, `safe-push --to main --wait`, which waits for the lander in the foreground by design.
fn is_hand_in(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let alone = !command.contains([';', '&', '|', '\n', '`', '$', '(']);
    alone && words.next().is_some_and(|program| program.rsplit('/').next() == Some("safe-push")) && words.any(|word| word == "--wait")
}

fn timeout_seconds(timeout: &Value) -> Option<f64> {
    match timeout {
        Value::Number { raw, .. } => raw.parse::<f64>().ok().map(|ms| ms / 1000.0),
        _ => None,
    }
}

pub(super) fn blocking(command: &str, timeout_seconds: Option<f64>) -> Option<String> {
    let mut reasons = Vec::new();
    if let Some(seconds) = timeout_seconds.filter(|s| *s > LIMIT_SECONDS) {
        reasons.push(format!("a timeout of {} s", seconds.floor() as u64));
    }
    let unquoted = shell::strip_heredocs(command);
    let inert = shell::blank_quotes(&unquoted);
    let bounded = SHORT_BOUND
        .captures(&unquoted)
        .is_some_and(|found| found[1].parse::<u64>().is_ok_and(|n| n as f64 <= LIMIT_SECONDS));
    for wait in WAITS.iter() {
        let text = if wait.inert { &inert } else { &unquoted };
        let hit = wait.pattern.captures_iter(text).any(|found| {
            found.get(1).is_none_or(|word| !SAFE_PUSH_QUERIES.contains(&word.as_str()))
        });
        if hit && (wait.inert || !bounded) {
            reasons.push(wait.label.to_owned());
        }
    }
    let runs_suite = TEST_SUITE
        .find_iter(&inert)
        .any(|found| !SINGLE_TEST_FILE.is_match(&inert[found.end()..]));
    if runs_suite {
        reasons.push("a test suite".to_owned());
    }
    reasons.extend(LONG_TIMEOUT.captures_iter(command).filter_map(|found| {
        let over = found[1].parse::<u64>().map_or(true, |n| n as f64 > LIMIT_SECONDS);
        over.then(|| format!("`{}`", &found[0]))
    }));
    (!reasons.is_empty()).then(|| reasons.join(" and "))
}
