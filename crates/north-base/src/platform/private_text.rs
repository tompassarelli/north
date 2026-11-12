use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use rustix::fs::{FlockOperation, flock};

use crate::foundation::hook_deadline::HOOK_TIMEOUT_SECS;

use super::north_binary::north_binary;
use super::process;

const JUDGE_LIMIT: Duration = Duration::from_secs(300);
const JUDGE_P90_SECS: u64 = 28;
const HOOK_MARGIN_SECS: u64 = 15;
const HOOK_LIMIT: Duration = Duration::from_secs(HOOK_TIMEOUT_SECS as u64 - HOOK_MARGIN_SECS);
const _: () = assert!(HOOK_LIMIT.as_secs() >= JUDGE_P90_SECS + 10, "the hook must wait past the judge's measured p90 (13 to 28 s on 2026-11-08) with room to spare");
const AT_ONCE_LIMIT: Duration = Duration::from_millis(300);
const JUDGE_ARGS: [&str; 5] = ["check", "privacy", "judge", "--text", "-"];
const SPAN_CHARS: usize = 160;
const JUDGE_SLOTS: usize = 4;
const SLOT_POLL: Duration = Duration::from_millis(100);
pub const KINDS_ENV: &str = "NORTH_PRIVATE_KINDS";
pub const AT_ONCE_ENV: &str = "NORTH_PRIVACY_AT_ONCE";
pub const ASKING_STATUS: i32 = 3;
pub const SEGMENT_BREAK: &str = "\u{1e}\n";
pub const CACHE_ENV: &str = "NORTH_PRIVACY_CACHE";
pub const CACHED: &str = "cache";
pub const RAN: &str = "private-kind judge ran: ";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Finding {
    pub kind: String,
    pub span: String,
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
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), std::path::PathBuf::from);
    let _slot = slot(&state.join("north/privacy-judge"));
    verdict(&process::run_command_within(command(), Some(text.as_bytes()), JUDGE_LIMIT).ok_or("the private-text judge did not finish")?)
}

pub fn judge_at_once(text: &str) -> Result<Option<Vec<Finding>>, String> {
    answered(process::run_command_or_leave(at_once(), text.as_bytes(), AT_ONCE_LIMIT))
}

pub fn judge_while(text: &str, running: &dyn Fn() -> bool) -> Result<Option<Vec<Finding>>, String> {
    let started = std::time::Instant::now();
    answered(process::run_command_or_leave_once(command(), text.as_bytes(), JUDGE_LIMIT, &mut || started.elapsed() >= AT_ONCE_LIMIT && !running()))
}

fn at_once() -> Command {
    let mut command = command();
    command.env(AT_ONCE_ENV, "1");
    command
}

fn answered(ran: std::io::Result<Option<Output>>) -> Result<Option<Vec<Finding>>, String> {
    match ran {
        Ok(Some(output)) if output.status.code() == Some(ASKING_STATUS) => Ok(None),
        Ok(Some(output)) => verdict(&output).map(Some),
        Ok(None) => Ok(None),
        Err(error) => Err(format!("the private-text judge did not start: {error}")),
    }
}

pub fn judge_within(text: &str, wait: Duration) -> Result<Option<Vec<Finding>>, String> {
    match process::run_command_or_leave(command(), text.as_bytes(), wait) {
        Ok(Some(output)) => verdict(&output).map(Some),
        Ok(None) => Ok(None),
        Err(error) => Err(format!("the private-text judge did not start: {error}")),
    }
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

/// Holds one of the host's `JUDGE_SLOTS` judge slots until dropped, so concurrent candidate checks wait in turn instead of each starting its own model runs; `None` when the slot directory cannot be made.
fn slot(directory: &Path) -> Option<JudgeSlot> {
    fs::create_dir_all(directory).ok()?;
    loop {
        if let Some(held) = try_slot(directory)? {
            return Some(held);
        }
        std::thread::sleep(SLOT_POLL);
    }
}

struct JudgeSlot(File);

impl Drop for JudgeSlot {
    fn drop(&mut self) {
        let _ = flock(&self.0, FlockOperation::Unlock);
    }
}

fn try_slot(directory: &Path) -> Option<Option<JudgeSlot>> {
    for index in 0..JUDGE_SLOTS {
        let file = OpenOptions::new().create(true).truncate(false).write(true).open(directory.join(format!("slot-{index}.lock"))).ok()?;
        if flock(&file, FlockOperation::NonBlockingLockExclusive).is_ok() {
            return Some(Some(JudgeSlot(file)));
        }
    }
    Some(None)
}

/// The directory the judge keeps its verdicts in, by chunk, and its receipts, by whole text.
pub fn verdict_dir() -> std::path::PathBuf {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os(CACHE_ENV).filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state/north/private-text-verdicts"), std::path::PathBuf::from)
}

/// Keeps which tiers ran and which judge in the chain answered for `text`, as labels that name no account, once a run asked a model for it.
pub fn write_receipt(text: &str, labels: &str) {
    let path = receipt_path(text);
    let line = format!("{}\t{labels}\n", seconds_now());
    let _ = fs::create_dir_all(verdict_dir()).and_then(|()| fs::write(&path, line));
}

/// What the last run that asked a model for `text` kept, when it asked at or after `since`; `cache` when no run since then asked one.
pub fn receipt(text: &str, since: f64) -> String {
    let held = fs::read_to_string(receipt_path(text)).unwrap_or_default();
    let fresh = held.trim_end().split_once('\t').filter(|(at, _)| at.parse::<f64>().is_ok_and(|at| at >= since));
    fresh.map_or_else(|| CACHED.to_owned(), |(_, labels)| labels.to_owned())
}

/// The tiers and the answering judge a verdict's lines name, which ride on them from where the judge ran to where the verdict is read.
pub fn ran(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| line.strip_prefix(RAN)).map(str::to_owned)
}

pub fn seconds_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

fn receipt_path(text: &str) -> std::path::PathBuf {
    let name: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(text.as_bytes())).iter().map(|byte| format!("{byte:02x}")).collect();
    verdict_dir().join(format!("{name}.ran"))
}

#[cfg(test)]
#[path = "private_text_tests.rs"]
mod tests;
