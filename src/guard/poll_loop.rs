use crate::hook::{HookInput, Verdict};
use crate::shell;

const LIMIT_SECONDS: f64 = 60.0;
const PREFIXES: &[&str] = &["exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "time"];
const KEYWORDS: &[&str] = &["do", "then", "else", "{", "(", "!"];

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("poll-loop-guard") {
        return Verdict::Allow;
    }
    verdict(input)
}

fn verdict(input: &HookInput) -> Verdict {
    if input.tool_input["run_in_background"].as_bool() == Some(true) {
        return Verdict::Allow;
    }
    let Some(command) = input.command().and_then(|s| s.as_str()) else {
        return Verdict::Allow;
    };
    let bound = ["timeout", "timeout_ms"]
        .iter()
        .find_map(|field| input.tool_input[*field].as_f64())
        .map_or(f64::INFINITY, |ms| ms / 1000.0);
    match decide(command, bound) {
        Some(reason) => Verdict::Deny(reason),
        None => Verdict::Allow,
    }
}

pub(super) fn decide(command: &str, bound: f64) -> Option<String> {
    let wait = waited(command, bound, 0);
    (wait > LIMIT_SECONDS).then(|| refusal(wait))
}

fn refusal(wait: f64) -> String {
    let span = if wait.is_finite() { format!("up to {wait:.0} s") } else { "until its condition holds".into() };
    format!(
        "No. This call waits {span} in a sleep or poll loop, and every wait over {LIMIT_SECONDS:.0} s holds a full-context request open. \
         For work that ends on its own, run `job-run --notify NAME -- CMD` and end your turn: its exit arrives as a fleet message. \
         To wake on a condition, use Monitor with an until-loop, or run the loop with run_in_background so its exit notifies you. \
         A sleep or a loop bounded by `timeout` at {LIMIT_SECONDS:.0} s or less runs inline."
    )
}

fn waited(source: &str, bound: f64, depth: usize) -> f64 {
    let Some(tokens) = shell::words(&shell::strip_heredocs(source)) else {
        return 0.0;
    };
    let mut loops: Vec<bool> = Vec::new();
    let mut sleeping = 0.0;
    let mut polling: f64 = 0.0;
    for segment in shell::segments(&tokens) {
        let start = segment.iter().take_while(|word| KEYWORDS.contains(&word.as_str())).count();
        let words = &segment[start..];
        match words.first().map(String::as_str) {
            Some("while" | "until") => {
                loops.push(false);
                continue;
            }
            Some("done") => {
                if loops.pop() == Some(true) {
                    polling = polling.max(bound);
                }
                continue;
            }
            _ => {}
        }
        let (words, limit) = bounded(words, bound);
        let Some(name) = words.first().map(|word| shell::basename(word)) else {
            continue;
        };
        let wait = match name {
            "sleep" => words[1..].iter().filter_map(|word| seconds(word)).sum::<f64>().min(limit),
            "bash" | "sh" if depth < 8 => nested(words).map_or(0.0, |source| waited(&source, limit, depth + 1)),
            _ => 0.0,
        };
        match loops.last_mut() {
            Some(sleeps) if name == "sleep" => *sleeps = true,
            Some(_) => {}
            None if name == "sleep" => sleeping += wait,
            None => polling = polling.max(wait),
        }
    }
    if loops.iter().any(|sleeps| *sleeps) {
        polling = polling.max(bound);
    }
    polling.max(sleeping.min(bound))
}

fn bounded(segment: &[String], bound: f64) -> (&[String], f64) {
    let mut limit = bound;
    let mut index = 0;
    loop {
        index += shell::command(&segment[index..], PREFIXES);
        match segment.get(index).map(|word| shell::basename(word)) {
            Some("timeout") => {
                index += 1;
                while segment.get(index).is_some_and(|word| word.starts_with('-')) {
                    index += 1;
                }
                if let Some(seconds) = segment.get(index).and_then(|word| seconds(word)) {
                    limit = limit.min(seconds);
                    index += 1;
                }
            }
            _ => return (&segment[index..], limit),
        }
    }
}

fn nested(words: &[String]) -> Option<String> {
    let flag = words.iter().skip(1).position(|word| word.starts_with('-') && word[1..].contains('c'))? + 1;
    words.get(flag + 1).cloned()
}

fn seconds(word: &str) -> Option<f64> {
    if word == "infinity" {
        return Some(f64::INFINITY);
    }
    let (number, unit) = match word.char_indices().last()? {
        (at, 's') => (&word[..at], 1.0),
        (at, 'm') => (&word[..at], 60.0),
        (at, 'h') => (&word[..at], 3600.0),
        (at, 'd') => (&word[..at], 86400.0),
        _ => (word, 1.0),
    };
    number.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0).map(|n| n * unit)
}

#[cfg(test)]
#[path = "poll_loop_tests.rs"]
mod tests;
