use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::platform::git as platform_git;
use crate::test_file::is_test_file;

const REMEDY: &str = "drive the time under test through its injected clock (NORTH_CAPACITY_DEADLINE_CLOCK for a deadline, a clock argument or a release file elsewhere) and wait on the state the test needs; a sleep belongs only inside a loop that polls a condition";

static SLEEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[^\w.])(?:(?:std::)?thread::|(?:tokio::)?time::)?sleep(?:_until)?\(").expect("sleep pattern"));
static ELAPSED_VERDICT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bassert(?:_[a-z]+)?!\(.*(?:\.elapsed\(\)|Instant::now\(\)|SystemTime::now\(\)).*(?:<|>)").expect("elapsed verdict pattern")
});
static LOOP_HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:'\w+:\s*)?(?:loop|while|for)\b").expect("loop pattern"));
static FUNCTION_HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\b").expect("function pattern"));

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn polled(lines: &[&str], at: usize) -> bool {
    let mut level = indent(lines[at]);
    for line in lines[..at].iter().rev().filter(|line| !line.trim().is_empty()) {
        if indent(line) >= level {
            continue;
        }
        level = indent(line);
        let head = line.trim_start();
        if LOOP_HEAD.is_match(head) {
            return true;
        }
        if FUNCTION_HEAD.is_match(head) {
            return false;
        }
    }
    false
}

fn races(line: &str) -> Option<&'static str> {
    let code = line.trim_start();
    if code.starts_with("//") {
        return None;
    }
    if SLEEP.is_match(code) {
        Some("sleeps outside a polling loop")
    } else if ELAPSED_VERDICT.is_match(code) {
        Some("asserts on elapsed wall-clock time outside a polling loop")
    } else {
        None
    }
}

pub fn problems(path: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| races(line).filter(|_| !polled(&lines, at)).map(|what| (at, what)))
        .map(|(at, what)| format!("safe-push: ✗ {path}:{} {what}: {} — {REMEDY}; NOT pushed.\n", at + 1, lines[at].trim()))
        .collect()
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    platform_git::run(repo, args).ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn scan(repo: &Path, base: &str, head: &str, out: &mut impl Write) -> u8 {
    let changed = git(repo, &["diff", "--name-only", "--no-renames", "--diff-filter=AM", base, head]).unwrap_or_default();
    let mut refused = false;
    for path in changed.lines().filter(|path| path.ends_with(".rs") && is_test_file(path)) {
        let Some(text) = git(repo, &["show", &format!("{head}:{path}")]) else { continue };
        for problem in problems(path, &text) {
            refused = true;
            let _ = out.write_all(problem.as_bytes());
        }
    }
    u8::from(refused)
}

#[cfg(test)]
#[path = "wall_clock_gate_tests.rs"]
mod tests;
