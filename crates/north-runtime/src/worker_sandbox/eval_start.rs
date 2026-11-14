//! A start script from a registered evaluation directory, run as its own user unit by the outside broker for a lead or the coordinator whose domain reaches neither ssh nor the user manager's bus (north#1439). The request is `eval-unit start script DIR/NAME LABEL [ARGUMENT...]`: the script must be a file git tracks unchanged under a directory the harness default `eval_start_directories` lists, and must name the north runtime it measures (`# eval-runtime: SHA` or `any`) and the hosts it holds (`# eval-holds: HOST...` or `none`). The broker refuses a runtime other than the one this host runs and a host a running start already holds, records each start, and answers the unit's name and log path; its state answer carries the log's last lines, since the caller cannot read the evaluation data directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::platform::harness::{DEFAULTS, document};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::eval_units::{Label, Unit, Units, plain, search_path};
use super::lane_grant::Host;

const DIRECTORIES: &str = "eval_start_directories";
const UNIT_PREFIX: &str = "eval-start-";
const STARTS: &str = "eval-data/starts";
const RECORD: &str = "starts.jsonl";
const RUNTIME_PIN: &str = ".local/state/north/current/built-from";
const RUNTIME_LINE: &str = "# eval-runtime:";
const HOLDS_LINE: &str = "# eval-holds:";
const ANY_RUNTIME: &str = "any";
const NO_HOLDS: &str = "none";
const HEADER_LINES: usize = 30;
const SEGMENTS: usize = 4;
const ARGUMENTS: usize = 16;
const ARGUMENT_LIMIT: usize = 200;
const TAIL: usize = 10;
const RUNNING: [&str; 4] = ["active", "activating", "deactivating", "reloading"];

/// A start script named relative to a registered evaluation directory, such as `b/start-paired`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Script(String);

impl Script {
    pub(super) fn parse(word: &str) -> Option<Self> {
        let segment = |part: &str| !part.is_empty() && !part.starts_with('.') && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
        let parts: Vec<&str> = word.split('/').collect();
        (parts.len() <= SEGMENTS && parts.iter().all(|part| segment(part))).then(|| Self(word.to_owned()))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Start {
    pub script: Script,
    pub label: Label,
    pub arguments: Vec<String>,
}

impl Start {
    pub(super) fn parse(script: &str, label: &str, arguments: &[&str]) -> Option<Self> {
        let argument = |word: &&str| !word.is_empty() && word.len() <= ARGUMENT_LIMIT && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._:/=,+-".contains(&byte));
        let fits = arguments.len() <= ARGUMENTS && arguments.iter().all(argument);
        fits.then_some(())?;
        Some(Self { script: Script::parse(script)?, label: Label::parse(label)?, arguments: arguments.iter().map(|word| (*word).to_owned()).collect() })
    }
}

fn directories(host: &Host) -> Vec<PathBuf> {
    document(DEFAULTS)[DIRECTORIES]["value"].as_array().into_iter().flatten().filter_map(|entry| entry.as_str()).map(|entry| host.home.join(entry)).collect()
}

fn git(directory: &Path, host: &Host, arguments: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git").arg("-C").arg(directory).args(arguments).env_clear().envs(host.environment.iter().cloned()).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

/// The registered file `script` names under one of `roots`, with its bytes: a regular executable file reached through no link, tracked by git and unchanged against its checkout's head.
fn registered(script: &Script, roots: &[PathBuf], host: &Host) -> Result<(PathBuf, Vec<u8>), String> {
    let (root, path) = roots.iter().map(|root| (root, root.join(&script.0))).find(|(_, path)| path.symlink_metadata().is_ok()).ok_or_else(|| format!("{} is in no registered evaluation directory ({DIRECTORIES})", script.0))?;
    if path.canonicalize().ok().as_ref() != Some(&path) {
        return Err(format!("{} is reached through a link", path.display()));
    }
    let executable = path.metadata().is_ok_and(|file| file.is_file() && std::os::unix::fs::PermissionsExt::mode(&file.permissions()) & 0o100 != 0);
    if !executable {
        return Err(format!("{} is no executable file", path.display()));
    }
    let tracked = git(root, host, &["ls-files", "--error-unmatch", "--", &script.0])?;
    if !tracked.status.success() {
        return Err(format!("{} is not registered: git tracks no such file", path.display()));
    }
    let clean = git(root, host, &["status", "--porcelain", "--", &script.0])?;
    if !clean.status.success() || !clean.stdout.is_empty() {
        return Err(format!("{} differs from its checkout's head, so it is not the registered script", path.display()));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((path, bytes))
}

/// The runtime and the holds the script's header names.
fn header(path: &Path, bytes: &[u8]) -> Result<(String, Vec<String>), String> {
    let text = String::from_utf8_lossy(bytes);
    let line = |prefix: &str| text.lines().take(HEADER_LINES).find_map(|line| line.strip_prefix(prefix)).map(|rest| rest.split_whitespace().map(str::to_owned).collect::<Vec<_>>());
    let runtime = line(RUNTIME_LINE).and_then(|words| <[String; 1]>::try_from(words).ok()).map(|[word]| word).filter(|word| word == ANY_RUNTIME || (word.len() >= 7 && word.bytes().all(|byte| byte.is_ascii_hexdigit())));
    let runtime = runtime.ok_or_else(|| format!("{} names no runtime: give a line `{RUNTIME_LINE} SHA` (or `{ANY_RUNTIME}`) in its first {HEADER_LINES} lines", path.display()))?;
    let holds = line(HOLDS_LINE).filter(|words| !words.is_empty()).ok_or_else(|| format!("{} names no holds: give a line `{HOLDS_LINE} HOST...` (or `{NO_HOLDS}`) in its first {HEADER_LINES} lines", path.display()))?;
    let holds = if holds == [NO_HOLDS] { Vec::new() } else { holds };
    if holds.iter().any(|host| !host.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')) {
        return Err(format!("{} holds a host whose name is not one", path.display()));
    }
    Ok((runtime, holds))
}

fn current_runtime(host: &Host) -> Result<String, String> {
    let pin = host.home.join(RUNTIME_PIN);
    std::fs::read_to_string(&pin).ok().and_then(|text| text.split_whitespace().next().map(str::to_owned)).ok_or_else(|| format!("{} names no runtime", pin.display()))
}

fn records(host: &Host) -> Vec<Value> {
    std::fs::read_to_string(host.home.join(STARTS).join(RECORD)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

fn unit_name(label: &Label) -> String {
    format!("{UNIT_PREFIX}{}", label.word())
}

/// The last start recorded for `label`.
pub(super) fn recorded(label: &Label, host: &Host) -> Option<Value> {
    records(host).into_iter().rev().find(|record| record["label"].as_str() == Some(label.word()))
}

fn running(units: &dyn Units, unit: &str) -> bool {
    units.show(unit).is_ok_and(|shown| RUNNING.contains(&shown.state.as_str()))
}

/// Starts `start` for the session `parent` on `host` with the user manager `units`; the line naming the unit and its log.
pub(super) fn serve(start: Start, parent: &str, host: &Host, units: &dyn Units) -> Result<String, String> {
    let (path, bytes) = registered(&start.script, &directories(host), host)?;
    let (runtime, holds) = header(&path, &bytes)?;
    let current = current_runtime(host)?;
    if runtime != ANY_RUNTIME && !current.starts_with(&runtime) {
        return Err(format!("{} measures the runtime {runtime}, and this host runs {}", start.script.0, &current[..current.len().min(10)]));
    }
    let name = unit_name(&start.label);
    if running(units, &name) {
        return Err(format!("{name} is already running"));
    }
    for record in records(host) {
        let held: Vec<&str> = record["holds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if let Some(shared) = holds.iter().find(|host| held.contains(&host.as_str()))
            && let Some(other) = record["unit"].as_str().filter(|other| running(units, other))
        {
            return Err(format!("{other} is running and holds {shared}, which {} holds too", start.script.0));
        }
    }
    let starts = host.home.join(STARTS);
    std::fs::create_dir_all(&starts).map_err(|error| format!("{}: {error}", starts.display()))?;
    let log = starts.join(format!("{}.log", start.label.word()));
    let agent = host.environment.iter().filter(|(name, value)| name == "SSH_AUTH_SOCK" && Path::new(value).is_absolute()).cloned();
    let environment = [("PATH".to_owned(), search_path(host)?), ("EVAL_START_ASKER".to_owned(), parent.to_owned()), ("EVAL_START_LABEL".to_owned(), start.label.word().to_owned())].into_iter().chain(agent).collect();
    let command = std::iter::once(plain(&path)?).chain(start.arguments.iter().cloned()).collect();
    let unit = Unit { name: name.clone(), description: format!("north eval-unit script {} for {parent}", start.script.0), environment, log: log.clone(), command, after: Vec::new() };
    units.start(&unit)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let row = json!({ "at": at, "by": parent, "script": start.script.0, "sha256": digest, "arguments": start.arguments, "label": start.label.word(), "unit": name, "log": log, "runtime": current, "holds": holds });
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(starts.join(RECORD)).map_err(|error| format!("{}: {error}", starts.join(RECORD).display()))?;
    std::io::Write::write_all(&mut file, format!("{row}\n").as_bytes()).map_err(|error| format!("{}: {error}", starts.join(RECORD).display()))?;
    Ok(format!("{name} started from {} (sha256 {digest}); log {}", start.script.0, log.display()))
}

/// The state of the recorded start `record` and the last lines of its log.
pub(super) fn state(record: &Value, units: &dyn Units) -> Result<String, String> {
    let unit = record["unit"].as_str().ok_or("the start record names no unit")?;
    let shown = units.show(unit)?;
    let log = record["log"].as_str().map(|log| std::fs::read_to_string(log).unwrap_or_default()).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let tail = lines[lines.len().saturating_sub(TAIL)..].join("\n");
    Ok(format!("{unit}: {}, result {}; log {}\n{tail}", shown.state, shown.result, record["log"].as_str().unwrap_or_default()))
}

/// Stops the recorded start `record`.
pub(super) fn stop(record: &Value, units: &dyn Units) -> Result<String, String> {
    let unit = record["unit"].as_str().ok_or("the start record names no unit")?;
    units.stop(unit)?;
    Ok(format!("{unit}: stopped"))
}

#[cfg(test)]
#[path = "eval_start_tests.rs"]
mod tests;
