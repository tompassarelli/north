//! The coverage map each host's graph holds: a host whose `coverage:REPO` commit differs from the
//! lander's cannot prove a test unaffected from that map, so the lander names it on the round line
//! (north#1039). The line is checked every few minutes, not on every tight serve round.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use north_base::platform::{host, north_binary, orchestration, process, push_ledger};

use super::candidate::git;

const CHECK_EVERY: Duration = Duration::from_secs(300);
const SSH_WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub host: String,
    pub commit: Option<String>,
}

/// The round line naming each host whose graph holds a different map than the lander's, with no
/// line when every host holds the same commit.
pub fn note(lander: &str, held: &[Held]) -> Option<String> {
    let differs: Vec<String> = held
        .iter()
        .filter_map(|host| match &host.commit {
            Some(commit) if commit == lander => None,
            Some(commit) => Some(format!("{} holds {}, not the lander's {}", host.host, short(commit), short(lander))),
            None => Some(format!("{} holds no coverage map, unlike the lander's {}", host.host, short(lander))),
        })
        .collect();
    (!differs.is_empty()).then(|| format!("coverage maps: {}", differs.join("; ")))
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// The line of the lander serving `repo`; `None` from a repository whose own graph holds no map
/// and from a host no test build reaches.
pub fn report(repo: &Path) -> Option<String> {
    let term = term(repo)?;
    let lander = commit_here(&term).ok()??;
    let here = host::name();
    let mut held = Vec::new();
    for (name, destination) in build_hosts() {
        if name == here {
            continue;
        }
        if let Ok(commit) = commit_on(&destination, &term) {
            held.push(Held { host: name, commit });
        }
    }
    note(&lander, &held)
}

fn term(repo: &Path) -> Option<String> {
    let origin = git(repo, &["remote", "get-url", "origin"]).ok()?;
    let name = push_ledger::repository(&origin);
    (!name.is_empty() && name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))).then(|| format!("coverage:{name}"))
}

fn build_hosts() -> Vec<(String, String)> {
    orchestration::read().map(|cfg| orchestration::build_hosts(&cfg)).unwrap_or_default()
}

fn commit_here(term: &str) -> Result<Option<String>, String> {
    let mut command = Command::new(north_binary::north_binary());
    command.args(["graph", "show", "--json", term]).stdin(Stdio::null());
    output(command, "this host's graph")
}

fn commit_on(destination: &str, term: &str) -> Result<Option<String>, String> {
    let script = format!("n=\"${{AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}}\" && \"$n\" graph show --json {}", quote(term));
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2", "-T", "--", destination, &script]).stdin(Stdio::null());
    output(command, destination)
}

fn output(command: Command, what: &str) -> Result<Option<String>, String> {
    match process::try_run_command_traced_within(command, None, SSH_WAIT) {
        Ok(Ok(output)) if output.status.success() => Ok(commit_of(&String::from_utf8_lossy(&output.stdout))),
        Ok(Ok(output)) => Err(format!("{what}: {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim())),
        Ok(Err(stalled)) => Err(format!("{what} gave no answer within {} s and was killed: {stalled}", SSH_WAIT.as_secs())),
        Err(error) => Err(format!("{what}: {error}")),
    }
}

/// The object of the `commit` triple in `north graph show --json`'s one-object-a-line output.
fn commit_of(output: &str) -> Option<String> {
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(triple) = value.get("triple").and_then(serde_json::Value::as_array) else { continue };
        let [serde_json::Value::String(kind), _, predicate, object] = triple.as_slice() else { continue };
        if kind != "triple" || string_term(predicate) != Some("commit") {
            continue;
        }
        return string_term(object).map(str::to_owned);
    }
    None
}

fn string_term(term: &serde_json::Value) -> Option<&str> {
    match term.as_array().map(Vec::as_slice) {
        Some([serde_json::Value::String(kind), serde_json::Value::String(text)]) if kind == "string" => Some(text.as_str()),
        _ => None,
    }
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The round line, checked at most every [`CHECK_EVERY`] so a tight serve loop does not ssh on every round.
#[derive(Default)]
pub struct Watch {
    checked: Option<Instant>,
    line: Option<String>,
}

impl Watch {
    pub fn line(&mut self, repo: &Path) -> Option<String> {
        if self.checked.is_some_and(|at| at.elapsed() < CHECK_EVERY) {
            return self.line.clone();
        }
        self.checked = Some(Instant::now());
        self.line = report(repo);
        self.line.clone()
    }
}

#[cfg(test)]
#[path = "map_lag_tests.rs"]
mod tests;
