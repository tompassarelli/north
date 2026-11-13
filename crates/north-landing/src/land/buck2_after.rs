//! The Buck2 run after a landing (north#1115): `north land crate-tests after-landing` on the landed head, in its own
//! offload slot on a build host beside the Cargo run after the landing, whose job and timing it never reads or changes.
//! Its exit status alone is its verdict: 0 green, 1 red, 75 none. It records one row per head (head, host, exit,
//! seconds, crates Buck2 passed) and the crates the job marked passed, one per line. A red reverts the landing at once
//! (the lander asks `reverts_at_once`); the Cargo run after the landing still decides its own revert. When the Buck2 and
//! Cargo verdicts of a head disagree, whichever of the two ends last records the disagreement once and tells the proxy.
//! When the Cargo run after the same landing is red on a case of a crate Buck2 passed, whichever of the two ends last
//! writes the trip record (crate and case, one pair a line) and tells the proxy; the trip turns nothing off.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;

use north_base::foundation::check_log::log_text;
use north_base::foundation::offload::{AFTER_LANDING_OWNER, OWNER_ENV};
use north_base::platform::harness::{DEFAULTS, document};

use super::after::follow;
use super::slots::listed_crates::{AFTER_LANDING, NO_VERDICT, PASSED};
use super::wake::{Task, Wake};
use crate::landing::check_timings::Timings;

const SWITCH: &str = "buck2_after_landing";
const RECORD: &str = "buck2-after-landing";
const GREEN: i32 = 0;
const RED: i32 = 1;
/// The slowest Buck2 workspace test line of a land check on 2026-11-13 took 1257 s (cpx62b).
const TIMEOUT_SECONDS: u64 = 1800;

pub struct Finished {
    pub exit: i32,
    pub log: PathBuf,
}

pub fn enabled() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}

pub fn reverts_at_once(exit: i32, base: &str) -> bool {
    reverts(exit, verdict_of(&Timings::default_root(), base).as_deref())
}

fn reverts(exit: i32, base: Option<&str>) -> bool {
    matches!((exit, base), (RED, Some("green")))
}

fn verdict_of(root: &Path, head: &str) -> Option<String> {
    fs::read_to_string(root.join(RECORD).join(format!("{head}.buck2"))).ok().map(|word| word.trim().to_owned())
}

/// Starts the Buck2 run of `head` on its own thread; starts nothing when the setting is off.
pub fn start(wake: &Arc<Wake>, repo: &Path, queue: &Path, offload_owner: &str, head: &str) -> Option<Task<Finished>> {
    if !enabled() {
        return None;
    }
    let (repo, logs, owner, head) = (repo.to_owned(), queue.join("logs"), format!("{offload_owner}{AFTER_LANDING_OWNER}"), head.to_owned());
    Some(Task::spawn(wake, move || run(&repo, &logs, &owner, &head)))
}

fn run(repo: &Path, logs: &Path, owner: &str, head: &str) -> Finished {
    let log = logs.join(format!("{RECORD}-{}.log", &head[..head.len().min(12)]));
    let Some(host) = build_host(repo) else {
        println!("north land: no build host answered, so Buck2 does not test {head} after its landing");
        return Finished { exit: i32::from(NO_VERDICT), log };
    };
    println!("north land: Buck2 tests {head} after its landing on {host}");
    let (name, value) = AFTER_LANDING;
    let command = format!("{name}={value} north land crate-tests after-landing");
    let stderr = fs::create_dir_all(logs).and_then(|()| fs::File::create(&log)).map_or_else(|_| Stdio::null(), Stdio::from);
    let started = Instant::now();
    let ran = Command::new(follow::program())
        .args(["capacity", "offload", "run", "--host", &host, "--base", head, "--head", head, "--timeout", &TIMEOUT_SECONDS.to_string(), "--", &command])
        .current_dir(repo)
        .env(OWNER_ENV, owner)
        .stdin(Stdio::null())
        .stderr(stderr)
        .output();
    let seconds = started.elapsed().as_secs();
    let (exit, printed) = match ran {
        Ok(output) => (verdict(output.status.code(), &log), String::from_utf8_lossy(&output.stdout).into_owned()),
        Err(error) => {
            println!("north land: could not start the Buck2 run of {head} on {host}: {error}");
            (i32::from(NO_VERDICT), String::new())
        }
    };
    let crates: Vec<&str> = if exit == GREEN { printed.lines().filter_map(|line| line.strip_prefix(PASSED)).collect() } else { Vec::new() };
    let passed = crates.len();
    println!("north land: Buck2 after the landing of {head} on {host} exited {exit} in {seconds} s with {passed} crate(s) passed; log: {}", log.display());
    let root = Timings::default_root();
    if let Err(error) = remember(&root, head, &host, exit, seconds, &crates) {
        println!("north land: could not record the Buck2 run of {head}: {error}");
    }
    buck2_settled(&root, head, exit, follow::send);
    trip(&root, head, follow::send);
    Finished { exit, log }
}

/// A build host the offload chooses, never this host: the run starts nothing rather than test here.
fn build_host(repo: &Path) -> Option<String> {
    let chosen = Command::new(follow::program()).args(["capacity", "offload", "choose", "--any"]).current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let host = String::from_utf8_lossy(&chosen.stdout).trim().to_owned();
    (!host.is_empty() && host != north_base::platform::host::name()).then_some(host)
}

fn verdict(code: Option<i32>, log: &Path) -> i32 {
    match code {
        Some(GREEN) => GREEN,
        Some(RED) if names_a_failing_case(log) => RED,
        _ => i32::from(NO_VERDICT),
    }
}

fn names_a_failing_case(log: &Path) -> bool {
    let text = fs::read_to_string(log).unwrap_or_default();
    let unstamped: Vec<&str> = text.lines().map(|line| line.split_once("] ").filter(|(stamp, _)| stamp.starts_with('[')).map_or(line, |(_, rest)| rest)).collect();
    !failed_cases(&unstamped.join("\n")).is_empty()
}

fn remember(root: &Path, head: &str, host: &str, exit: i32, seconds: u64, crates: &[&str]) -> std::io::Result<()> {
    let dir: PathBuf = root.join(RECORD);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{head}.crates")), crates.iter().map(|name| format!("{name}\n")).collect::<String>())?;
    let mut rows = OpenOptions::new().create(true).append(true).open(root.join(format!("{RECORD}.tsv")))?;
    writeln!(rows, "{head}\t{host}\t{exit}\t{seconds}\t{}", crates.len())
}

fn buck2_settled(root: &Path, head: &str, exit: i32, send: fn(&str, &str) -> bool) {
    let Some(word) = word(exit) else { return };
    settled(root, head, "buck2", word, send);
}

pub fn cargo_green(root: &Path, head: &str, send: fn(&str, &str) -> bool) {
    if !enabled() {
        return;
    }
    settled(root, head, "cargo", "green", send);
}

/// Records the cases the Cargo run after the landing of `head` failed, by crate, and trips when Buck2 passed one of their crates.
pub fn cargo_red(root: &Path, head: &str, log: &Path, send: fn(&str, &str) -> bool) {
    if !enabled() {
        return;
    }
    let cases: String = failed_cases(&fs::read_to_string(log).unwrap_or_default()).into_iter().map(|(name, case)| format!("{name}\t{case}\n")).collect();
    let dir = root.join(RECORD);
    if let Err(error) = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join(format!("{head}.red")), cases)) {
        println!("north land: could not record the Cargo cases that failed after the landing of {head}: {error}");
    }
    settled(root, head, "cargo", "red", send);
    trip(root, head, send);
}

fn word(exit: i32) -> Option<&'static str> {
    match exit {
        GREEN => Some("green"),
        RED => Some("red"),
        _ => None,
    }
}

fn settled(root: &Path, head: &str, side: &str, verdict: &str, send: fn(&str, &str) -> bool) {
    let dir = root.join(RECORD);
    if let Err(error) = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join(format!("{head}.{side}")), verdict)) {
        println!("north land: could not record the {side} verdict of {head}: {error}");
        return;
    }
    disagree(root, head, send);
}

fn disagree(root: &Path, head: &str, send: fn(&str, &str) -> bool) {
    let dir = root.join(RECORD);
    let (Ok(buck2), Ok(cargo)) = (fs::read_to_string(dir.join(format!("{head}.buck2"))), fs::read_to_string(dir.join(format!("{head}.cargo")))) else { return };
    if buck2 == cargo {
        return;
    }
    let Ok(_marker) = OpenOptions::new().write(true).create_new(true).open(dir.join(format!("{head}.disagree"))) else { return };
    let record = root.join(format!("{RECORD}-disagreements.tsv"));
    let written = OpenOptions::new().create(true).append(true).open(&record).and_then(|mut file| writeln!(file, "{head}\tbuck2 {buck2}\tcargo {cargo}"));
    let message = format!("north land: Buck2 and Cargo disagree on {head}: Buck2 {buck2}, Cargo {cargo}; record {}", record.display());
    println!("{message}");
    if written.is_err() || !send("proxy", &message) {
        println!("north land: the disagreement on {head} was not fully recorded or not sent to the proxy");
    }
}

/// The trip: a case of the Cargo run after the landing of `head` failed in a crate Buck2 passed at that head.
fn trip(root: &Path, head: &str, send: fn(&str, &str) -> bool) {
    let dir = root.join(RECORD);
    let (Ok(crates), Ok(red)) = (fs::read_to_string(dir.join(format!("{head}.crates"))), fs::read_to_string(dir.join(format!("{head}.red")))) else { return };
    let passed: BTreeSet<&str> = crates.lines().collect();
    let tripped: Vec<(&str, &str)> = red.lines().filter_map(|line| line.split_once('\t')).filter(|(name, _)| passed.contains(name)).collect();
    if tripped.is_empty() {
        return;
    }
    let record = dir.join(format!("{head}.trip"));
    let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(&record) else { return };
    let written = tripped.iter().try_for_each(|(name, case)| writeln!(file, "{name}\t{case}"));
    let named: Vec<String> = tripped.iter().map(|(name, case)| format!("{case} in {name}")).collect();
    let message = format!("north land: Buck2 passed a crate at {head} whose case the Cargo run after the landing failed: {}; trip record {}", named.join(", "), record.display());
    println!("{message}");
    if written.is_err() || !send("proxy", &message) {
        println!("north land: the Buck2 trip of {head} was not fully recorded or not sent to the proxy");
    }
}

/// The failed cases of a nextest log by crate: `FAIL [ TIME] (N/M) BINARY CASE`, where a binary names its crate before `::`.
fn failed_cases(log: &str) -> BTreeSet<(String, String)> {
    log.lines().map(log_text).filter_map(|line| {
        let (took, rest) = line.strip_prefix("FAIL [")?.split_once(']')?;
        took.trim().ends_with('s').then_some(())?;
        let mut words = rest.split_whitespace().skip_while(|word| word.starts_with('(') && word.ends_with(')'));
        let (binary, case) = (words.next()?, words.next()?);
        Some((binary.split("::").next()?.to_owned(), case.to_owned()))
    }).collect()
}

#[cfg(test)]
#[path = "buck2_after_tests.rs"]
mod tests;
