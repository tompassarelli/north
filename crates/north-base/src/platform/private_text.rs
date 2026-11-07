use std::process::{Command, Output};
use std::time::Duration;

use crate::foundation::hook_deadline::HOOK_TIMEOUT_SECS;

use super::north_binary::north_binary;
use super::process;

const JUDGE_LIMIT: Duration = Duration::from_secs(300);
const JUDGE_P90_SECS: u64 = 28;
const HOOK_MARGIN_SECS: u64 = 15;
const HOOK_LIMIT: Duration = Duration::from_secs(HOOK_TIMEOUT_SECS as u64 - HOOK_MARGIN_SECS);
const _: () = assert!(HOOK_LIMIT.as_secs() >= JUDGE_P90_SECS + 10, "the hook must wait past the judge's measured p90 (13 to 28 s on 2026-11-08) with room to spare");
const JUDGE_ARGS: [&str; 5] = ["check", "privacy", "judge", "--text", "-"];
const SPAN_CHARS: usize = 160;
pub const KINDS_ENV: &str = "NORTH_PRIVATE_KINDS";
pub const SEGMENT_BREAK: &str = "\u{1e}\n";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Finding {
    pub kind: String,
    pub span: String,
}

pub fn kinds_override() -> bool {
    std::env::var_os(KINDS_ENV).is_some_and(|value| !value.is_empty())
}

pub fn render(findings: &[Finding]) -> String {
    findings.iter().map(|finding| format!("{}\t{}\n", finding.kind, finding.span.replace(['\n', '\t'], " "))).collect()
}

pub fn parse(stdout: &str) -> Vec<Finding> {
    stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(kind, span)| Finding { kind: kind.to_owned(), span: span.to_owned() })
        .collect()
}

/// Judges `text` by running `north check privacy judge`: exit 0 is clean, exit 1 prints the findings, anything else means the judge could not run.
pub fn judge(text: &str) -> Result<Vec<Finding>, String> {
    verdict(&process::run_command_within(command(), Some(text.as_bytes()), JUDGE_LIMIT).ok_or("the private-text judge did not finish")?)
}

pub fn judge_in_hook(text: &str) -> Result<Vec<Finding>, String> {
    match process::run_command_or_leave(command(), text.as_bytes(), HOOK_LIMIT) {
        Ok(Some(output)) => verdict(&output),
        Ok(None) => Err("judging, retry the same command in about 15 s and the cached verdict answers it".to_owned()),
        Err(error) => Err(format!("the private-text judge did not start: {error}")),
    }
}

/// Whether `arguments` (a process's argv) are a private-text judge run, which a hook may leave running past its own wait.
pub fn is_judge_run(arguments: &[String]) -> bool {
    arguments.len() >= JUDGE_ARGS.len() && arguments[arguments.len() - JUDGE_ARGS.len()..].iter().map(String::as_str).eq(JUDGE_ARGS)
}

fn command() -> Command {
    let mut command = Command::new(north_binary());
    command.args(JUDGE_ARGS);
    command
}

fn verdict(output: &Output) -> Result<Vec<Finding>, String> {
    match output.status.code() {
        Some(0) => Ok(Vec::new()),
        Some(1) => Some(parse(&String::from_utf8_lossy(&output.stdout)))
            .filter(|found| !found.is_empty())
            .ok_or_else(|| "the private-text judge refused without naming a kind".to_owned()),
        code => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or("no reason given");
            let code = code.map_or_else(|| "signal".to_owned(), |code| code.to_string());
            Err(format!("the private-text judge could not run (exit {code}): {reason}"))
        }
    }
}

pub fn destination(finding: &Finding) -> String {
    let span: String = finding.span.chars().take(SPAN_CHARS).collect();
    format!("file it in south: {} (\"{span}\")", finding.kind)
}

pub fn refusal(findings: &[Finding]) -> String {
    let named: Vec<String> = findings.iter().map(destination).collect();
    format!("No. GitHub text is public and this text holds a private kind of detail; {}. Keep the public text generic.", named.join("; "))
}

pub fn unavailable(reason: &str) -> String {
    format!("No. GitHub text is public and the private-kind judge has no verdict on this text, so it is refused: {reason}.")
}
