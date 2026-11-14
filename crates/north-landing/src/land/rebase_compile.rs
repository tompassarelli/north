use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use north_base::foundation::offload::LANDER_SLOT_ENV;
use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::process;

use super::cargo_manifest::{self, Read};
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

fn north(program: &Path, tree: &Path, arguments: &[&str], limit: Duration) -> Result<(Option<i32>, String), String> {
    let mut command = Command::new(program);
    command.args(arguments).current_dir(tree).env(LANDER_SLOT_ENV, "1").stdin(Stdio::null());
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

pub enum Outcome {
    Judged(Option<Refused>),
    Unjudged(String),
}

fn unjudged(logs: &Path, name: &str, why: &str, log: &str) -> Result<Outcome, String> {
    let path = written(logs, name, &format!("{why}\n{log}"))?;
    Ok(Outcome::Unjudged(format!("north land: {name}: the compile after its rebase gave no verdict ({why}); it lands without one; log: {}", path.display())))
}

pub fn judge(rebased: &Rebased, program: &Path, logs: &Path, name: &str) -> Result<Option<Refused>, String> {
    Ok(match outcome(rebased, program, logs, name)? {
        Outcome::Judged(refused) => refused,
        Outcome::Unjudged(line) => {
            println!("{line}");
            None
        }
    })
}

pub fn outcome(rebased: &Rebased, program: &Path, logs: &Path, name: &str) -> Result<Outcome, String> {
    if !rebased.tree.join("Cargo.toml").is_file() {
        return Ok(Outcome::Judged(None));
    }
    let metadata = match cargo_manifest::read(rebased.tree, GRACE) {
        Read::Metadata(metadata) => metadata,
        Read::Unreadable { line, log, named } => return unreadable(rebased, logs, name, &format!("{line}\n{log}"), &named).map(|refused| Outcome::Judged(Some(refused))),
        Read::Unjudged { why, log } => return unjudged(logs, name, &why, &log),
    };
    let Some(checked_on) = rebased.checked_on.filter(|checked_on| *checked_on != rebased.picked_onto) else { return Ok(Outcome::Judged(None)) };
    match affected::rebase_scope(rebased.tree, &metadata, checked_on, rebased.picked_onto, rebased.head) {
        Ok(Some(scope)) => compiled(rebased, checked_on, &scope, program, (logs, name)),
        Ok(None) => Ok(Outcome::Judged(None)),
        Err(error) => unjudged(logs, name, &format!("cannot read which crates the rebase reaches: {error}"), ""),
    }
}

fn unreadable(rebased: &Rebased, logs: &Path, name: &str, log: &str, named: &[String]) -> Result<Refused, String> {
    let path = written(logs, name, log)?;
    let verdict = format!("failed {REBASE_COMPILE}: the lane rebased onto {} leaves a Cargo manifest cargo cannot read: {}; log: {}", &rebased.picked_onto[..rebased.picked_onto.len().min(10)], named.join("; "), path.display());
    Ok(Refused { gates: REBASE_COMPILE.to_owned(), verdict, log: path })
}

fn compiled(rebased: &Rebased, checked_on: &str, scope: &Scope, program: &Path, (logs, name): (&Path, &str)) -> Result<Outcome, String> {
    let chosen = match north(program, rebased.tree, &["capacity", "offload", "choose", "--any"], GRACE) {
        Ok((Some(0), printed)) => printed.lines().rev().map(str::trim).find(|line| !line.is_empty() && !line.contains(' ')).map(str::to_owned),
        Ok((_, printed)) => return unjudged(logs, name, "no build host was chosen", &printed),
        Err(error) => return unjudged(logs, name, &error, ""),
    };
    let Some(host) = chosen else { return unjudged(logs, name, "no build host was chosen", "") };
    let command = line(scope);
    let cap = CAP_SECONDS.to_string();
    let (code, log) = match north(program, rebased.tree, &["capacity", "offload", "run", "--host", &host, "--base", rebased.shipped_from, "--head", rebased.head, "--timeout", &cap, "--", &command], Duration::from_secs(CAP_SECONDS) + GRACE) {
        Ok(ran) => ran,
        Err(error) => return unjudged(logs, name, &error, ""),
    };
    if code == Some(0) {
        return Ok(Outcome::Judged(None));
    }
    let named = errors(&log);
    if code != Some(CARGO_FAILED) || named.is_empty() || !log.lines().any(|line| line.starts_with(UNCOMPILED)) {
        return unjudged(logs, name, &format!("`{command}` on {host} ended with {code:?} and named no compile error"), &log);
    }
    let path = written(logs, name, &format!("{command}\n{log}"))?;
    let verdict = format!(
        "failed {REBASE_COMPILE}: main moved from {} to {} since the lane's checks before queueing, and the lane rebased onto it does not compile: {}; log: {}",
        &checked_on[..checked_on.len().min(10)],
        &rebased.picked_onto[..rebased.picked_onto.len().min(10)],
        named.join("; "),
        path.display()
    );
    Ok(Outcome::Judged(Some(Refused { gates: REBASE_COMPILE.to_owned(), verdict, log: path })))
}

pub fn joined(gates: Option<Refused>, compile: Option<Refused>) -> Option<Refused> {
    match (gates, compile) {
        (Some(gates), Some(compile)) => Some(Refused { gates: format!("{}, {}", gates.gates, compile.gates), verdict: format!("{}; {}", gates.verdict, compile.verdict), log: gates.log }),
        (gates, compile) => gates.or(compile),
    }
}

#[derive(PartialEq, Eq, Hash)]
pub struct Ahead {
    pub lane_head: String,
    pub picked_onto: String,
    pub checked_on: String,
}

struct Ran {
    tree: String,
    outcome: Outcome,
}

#[derive(Default)]
struct Slot {
    ran: Mutex<Option<Ran>>,
    ready: Condvar,
}

fn slots() -> &'static Mutex<HashMap<Ahead, Arc<Slot>>> {
    static SLOTS: OnceLock<Mutex<HashMap<Ahead, Arc<Slot>>>> = OnceLock::new();
    SLOTS.get_or_init(Mutex::default)
}

pub struct Claim {
    slot: Arc<Slot>,
    ran: Option<Ran>,
}

impl Claim {
    pub fn ran(mut self, tree: String, outcome: Outcome) {
        self.ran = Some(Ran { tree, outcome });
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let ran = self.ran.take().unwrap_or(Ran { tree: String::new(), outcome: Outcome::Unjudged(String::new()) });
        *self.slot.ran.lock().unwrap_or_else(PoisonError::into_inner) = Some(ran);
        self.slot.ready.notify_all();
    }
}

pub fn claim(ahead: Ahead) -> Option<Claim> {
    let mut slots = slots().lock().unwrap_or_else(PoisonError::into_inner);
    slots.retain(|held, _| held.picked_onto == ahead.picked_onto);
    if slots.contains_key(&ahead) {
        return None;
    }
    let slot = Arc::<Slot>::default();
    slots.insert(ahead, Arc::clone(&slot));
    Some(Claim { slot, ran: None })
}

pub fn reused(ahead: &Ahead, tree: &str, name: &str) -> Option<Option<Refused>> {
    let slot = slots().lock().unwrap_or_else(PoisonError::into_inner).get(ahead).map(Arc::clone)?;
    let ran = slot.ready.wait_while(slot.ran.lock().unwrap_or_else(PoisonError::into_inner), |ran| ran.is_none()).unwrap_or_else(PoisonError::into_inner);
    match ran.as_ref() {
        Some(Ran { tree: compiled, outcome }) if compiled == tree => {
            println!("north land: {name}: reused the compile after its rebase onto {} that ran while it waited in the queue", &ahead.picked_onto[..ahead.picked_onto.len().min(10)]);
            Some(match outcome {
                Outcome::Judged(refused) => refused.clone(),
                Outcome::Unjudged(line) => {
                    println!("{line}");
                    None
                }
            })
        }
        _ => None,
    }
}
