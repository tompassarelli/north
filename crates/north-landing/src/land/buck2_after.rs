//! The Buck2 run after a landing (north#1115): `north land crate-tests after-landing` on the landed head, in its own
//! offload slot on a build host beside the Cargo run after the landing, whose job, verdict and timing it never reads or
//! changes. Its exit status alone is its verdict: 0 green, 1 red, 75 none. It records one row per head (head, host,
//! exit, seconds, crates Buck2 passed) and the crates the job marked passed, one per line; nothing decides on either.
//! When the Cargo run after the same landing is red on a case of a crate Buck2 passed, whichever of the two ends last
//! writes the trip record (crate and case, one pair a line) and tells the proxy; it turns nothing off.
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use north_base::foundation::check_log::log_text;
use north_base::foundation::offload::{AFTER_LANDING_OWNER, OWNER_ENV};
use north_base::platform::harness::{DEFAULTS, document};

use super::after::follow;
use super::slots::listed_crates::{AFTER_LANDING, NO_VERDICT, PASSED};
use crate::landing::check_timings::Timings;

const SWITCH: &str = "buck2_after_landing";
const RECORD: &str = "buck2-after-landing";
/// The slowest Buck2 workspace test line of a land check on 2026-11-13 took 1257 s (cpx62b).
const TIMEOUT_SECONDS: u64 = 1800;

pub fn enabled() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}

/// Starts the Buck2 run of `head` on its own thread; starts nothing when the setting is off.
pub fn start(repo: &Path, queue: &Path, offload_owner: &str, head: &str) {
    if !enabled() {
        return;
    }
    let (repo, logs, owner, head) = (repo.to_owned(), queue.join("logs"), format!("{offload_owner}{AFTER_LANDING_OWNER}"), head.to_owned());
    std::thread::spawn(move || run(&repo, &logs, &owner, &head));
}

fn run(repo: &Path, logs: &Path, owner: &str, head: &str) {
    let Some(host) = build_host(repo) else {
        println!("north land: no build host answered, so Buck2 does not test {head} after its landing");
        return;
    };
    println!("north land: Buck2 tests {head} after its landing on {host}");
    let (name, value) = AFTER_LANDING;
    let command = format!("{name}={value} north land crate-tests after-landing");
    let log = logs.join(format!("{RECORD}-{}.log", &head[..head.len().min(12)]));
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
        Ok(output) => (verdict(output.status.code()), String::from_utf8_lossy(&output.stdout).into_owned()),
        Err(error) => {
            println!("north land: could not start the Buck2 run of {head} on {host}: {error}");
            (i32::from(NO_VERDICT), String::new())
        }
    };
    let crates: Vec<&str> = if exit == 0 { printed.lines().filter_map(|line| line.strip_prefix(PASSED)).collect() } else { Vec::new() };
    let passed = crates.len();
    println!("north land: Buck2 after the landing of {head} on {host} exited {exit} in {seconds} s with {passed} crate(s) passed; log: {}", log.display());
    if let Err(error) = remember(&Timings::default_root(), head, &host, exit, seconds, &crates) {
        println!("north land: could not record the Buck2 run of {head}: {error}");
    }
    trip(&Timings::default_root(), head, follow::send);
}

/// A build host the offload chooses, never this host: the run starts nothing rather than test here.
fn build_host(repo: &Path) -> Option<String> {
    let chosen = Command::new(follow::program()).args(["capacity", "offload", "choose", "--any"]).current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let host = String::from_utf8_lossy(&chosen.stdout).trim().to_owned();
    (!host.is_empty() && host != north_base::platform::host::name()).then_some(host)
}

fn verdict(code: Option<i32>) -> i32 {
    match code {
        Some(code @ (0 | 1)) => code,
        _ => i32::from(NO_VERDICT),
    }
}

fn remember(root: &Path, head: &str, host: &str, exit: i32, seconds: u64, crates: &[&str]) -> std::io::Result<()> {
    let dir: PathBuf = root.join(RECORD);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{head}.crates")), crates.iter().map(|name| format!("{name}\n")).collect::<String>())?;
    let mut rows = OpenOptions::new().create(true).append(true).open(root.join(format!("{RECORD}.tsv")))?;
    writeln!(rows, "{head}\t{host}\t{exit}\t{seconds}\t{}", crates.len())
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
    trip(root, head, send);
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
