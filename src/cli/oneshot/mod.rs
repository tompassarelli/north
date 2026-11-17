//! The one-shot model call behind `north check oneshot`, the disclosure judge and the planner: one account's CLI asked one question.
use std::env;
use std::path::PathBuf;
use std::process::Output;


pub(super) mod ask;
mod billing;
pub(super) mod gemini;
pub(super) mod judge_model;
mod operational;
pub(super) mod session;

fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
}

fn env_or(name: &str, fallback: impl FnOnce() -> PathBuf) -> PathBuf {
    env::var_os(name).filter(|value| !value.is_empty()).map_or_else(fallback, PathBuf::from)
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn child_failure(name: &str, code: Option<i32>, stderr: &[u8]) -> String {
    let code = code.unwrap_or(1);
    let stderr = String::from_utf8_lossy(stderr);
    match stderr.lines().map(str::trim).rfind(|line| !line.is_empty()) {
        Some(line) => format!("{name} exited {code}: {line}"),
        None => format!("{name} exited {code}"),
    }
}
