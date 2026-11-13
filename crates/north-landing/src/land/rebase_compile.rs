use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::process;

use super::lane_gates::Refused;
use super::slots::affected::{self, Scope};
use crate::landing_refusals::REBASE_COMPILE;

const SWITCH: &str = "rebase_compile";
const CAP_SECONDS: u64 = 90;
const GRACE: Duration = Duration::from_secs(20);
const CARGO_FAILED: i32 = 101;
const UNCOMPILED: &str = "error: could not compile";
const NAMED_ERRORS: usize = 3;

pub fn enabled() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}

pub struct Rebased<'a> {
    pub tree: &'a Path,
    pub checked_on: Option<&'a str>,
    pub picked_onto: &'a str,
    pub head: &'a str,
    pub shipped_from: &'a str,
}

pub fn line(scope: &Scope) -> String {
    format!("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo check --all-targets --locked --offline {}", scope.arguments())
}

fn scope(rebased: &Rebased) -> Result<Option<Scope>, String> {
    match rebased.checked_on {
        Some(checked_on) if checked_on != rebased.picked_onto => affected::rebase_scope(rebased.tree, checked_on, rebased.picked_onto, rebased.head),
        _ => Ok(None),
    }
}

fn north(program: &Path, tree: &Path, arguments: &[&str], limit: Duration) -> Result<(Option<i32>, String), String> {
    let mut command = Command::new(program);
    command.args(arguments).current_dir(tree).stdin(Stdio::null());
    match process::try_run_group_traced_within(command, limit) {
        Ok(Ok(output)) => Ok((output.status.code(), format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))),
        Ok(Err(stalled)) => Err(format!("north {} gave no answer within {} s: {stalled}", arguments.join(" "), limit.as_secs())),
        Err(error) => Err(format!("north {}: {error}", arguments.join(" "))),
    }
}

fn errors(log: &str) -> Vec<String> {
    let lines: Vec<&str> = log.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("error[") || (line.starts_with("error: ") && !line.starts_with(UNCOMPILED)))
        .take(NAMED_ERRORS)
        .map(|(at, line)| match lines.get(at + 1).map(|next| next.trim_start()).filter(|next| next.starts_with("--> ")) {
            Some(location) => format!("{line} {location}"),
            None => (*line).to_owned(),
        })
        .collect()
}

fn written(logs: &Path, name: &str, text: &str) -> Result<PathBuf, String> {
    let path = logs.join(format!("{name}.rebase-compile.log"));
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

fn unjudged(logs: &Path, name: &str, why: &str, log: &str) -> Result<Option<Refused>, String> {
    let path = written(logs, name, &format!("{why}\n{log}"))?;
    println!("north land: {name}: the compile after its rebase gave no verdict ({why}); it lands without one; log: {}", path.display());
    Ok(None)
}

pub fn judge(rebased: &Rebased, program: &Path, logs: &Path, name: &str) -> Result<Option<Refused>, String> {
    let scope = match scope(rebased) {
        Ok(Some(scope)) => scope,
        Ok(None) => return Ok(None),
        Err(error) => return unjudged(logs, name, &format!("cannot read which crates the rebase reaches: {error}"), ""),
    };
    let chosen = match north(program, rebased.tree, &["capacity", "offload", "choose", "--any"], GRACE) {
        Ok((Some(0), printed)) => printed.lines().rev().map(str::trim).find(|line| !line.is_empty() && !line.contains(' ')).map(str::to_owned),
        Ok((_, printed)) => return unjudged(logs, name, "no build host was chosen", &printed),
        Err(error) => return unjudged(logs, name, &error, ""),
    };
    let Some(host) = chosen else { return unjudged(logs, name, "no build host was chosen", "") };
    let command = line(&scope);
    let cap = CAP_SECONDS.to_string();
    let (code, log) = match north(program, rebased.tree, &["capacity", "offload", "run", "--host", &host, "--base", rebased.shipped_from, "--head", rebased.head, "--timeout", &cap, "--", &command], Duration::from_secs(CAP_SECONDS) + GRACE) {
        Ok(ran) => ran,
        Err(error) => return unjudged(logs, name, &error, ""),
    };
    if code == Some(0) {
        return Ok(None);
    }
    let named = errors(&log);
    if code != Some(CARGO_FAILED) || named.is_empty() || !log.lines().any(|line| line.starts_with(UNCOMPILED)) {
        return unjudged(logs, name, &format!("`{command}` on {host} ended with {code:?} and named no compile error"), &log);
    }
    let path = written(logs, name, &format!("{command}\n{log}"))?;
    let checked_on = rebased.checked_on.unwrap_or_default();
    let verdict = format!(
        "failed {REBASE_COMPILE}: main moved from {} to {} since the lane's checks before queueing, and the lane rebased onto it does not compile: {}; log: {}",
        &checked_on[..checked_on.len().min(10)],
        &rebased.picked_onto[..rebased.picked_onto.len().min(10)],
        named.join("; "),
        path.display()
    );
    Ok(Some(Refused { gates: REBASE_COMPILE.to_owned(), verdict, log: path }))
}

pub fn joined(gates: Option<Refused>, compile: Option<Refused>) -> Option<Refused> {
    match (gates, compile) {
        (Some(gates), Some(compile)) => Some(Refused { gates: format!("{}, {}", gates.gates, compile.gates), verdict: format!("{}; {}", gates.verdict, compile.verdict), log: gates.log }),
        (gates, compile) => gates.or(compile),
    }
}
