//! The user units the outside broker starts for an evaluation lead, shared by the closed-list forms (eval_unit_request.rs) and the registered start scripts (eval_start.rs): the label that names a unit, the unit to start, the user manager that starts, shows and stops it, and the plain words a unit's command line carries.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::lane_grant::Host;

const LABEL_LIMIT: usize = 40;
pub(super) const NIX_PROFILE_BIN: &str = ".nix-profile/bin";
pub(super) const SYSTEM_BIN: &str = "/run/current-system/sw/bin";

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Label(String);

impl Label {
    pub(super) fn parse(word: &str) -> Option<Self> {
        let edge = |at: Option<char>| at.is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit());
        let fits = word.len() <= LABEL_LIMIT && edge(word.chars().next()) && edge(word.chars().last()) && word.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-');
        fits.then(|| Self(word.to_owned()))
    }

    pub(super) fn word(&self) -> &str {
        &self.0
    }
}

/// A transient unit to start: its name, the marker that says this request started it, its environment, its main command and the command that runs after it ends, whatever its result.
pub(super) struct Unit {
    pub name: String,
    pub description: String,
    pub environment: Vec<(String, String)>,
    pub log: PathBuf,
    pub command: Vec<String>,
    pub after: Vec<String>,
}

pub(super) struct Shown {
    pub id: String,
    pub description: String,
    pub state: String,
    pub result: String,
}

/// The user manager as the broker reaches it; the tests replace it.
pub(super) trait Units {
    fn start(&self, unit: &Unit) -> Result<(), String>;
    fn show(&self, name: &str) -> Result<Shown, String>;
    fn stop(&self, name: &str) -> Result<(), String>;
}

pub(super) struct UserManager;

fn run(program: &str, arguments: &[String]) -> Result<String, String> {
    let output = Command::new(program).args(arguments).output().map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!("{program} {}: {}", arguments.first().map_or("", String::as_str), String::from_utf8_lossy(&output.stderr).trim()))
    }
}

impl Units for UserManager {
    fn start(&self, unit: &Unit) -> Result<(), String> {
        let log = format!("append:{}", unit.log.display());
        let mut arguments: Vec<String> = ["--user", "--quiet", "--unit", &unit.name, "--description", &unit.description].map(str::to_owned).to_vec();
        arguments.extend(unit.environment.iter().map(|(name, value)| format!("--setenv={name}={value}")));
        arguments.extend([format!("--property=StandardOutput={log}"), format!("--property=StandardError={log}")]);
        arguments.extend((!unit.after.is_empty()).then(|| format!("--property=ExecStopPost={}", unit.after.join(" "))));
        arguments.push("--".to_owned());
        let _ = run("systemctl", &["--user", "reset-failed", &unit.name].map(str::to_owned));
        arguments.extend(unit.command.iter().cloned());
        run("systemd-run", &arguments).map(|_| ())
    }

    fn show(&self, name: &str) -> Result<Shown, String> {
        let said = run("systemctl", &["--user", "show", "-p", "Id", "-p", "Description", "-p", "ActiveState", "-p", "Result", name].map(str::to_owned))?;
        let field = |key: &str| said.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('=')).unwrap_or_default().to_owned();
        Ok(Shown { id: field("Id"), description: field("Description"), state: field("ActiveState"), result: field("Result") })
    }

    fn stop(&self, name: &str) -> Result<(), String> {
        run("systemctl", &["--user", "stop", name].map(str::to_owned))?;
        let _ = run("systemctl", &["--user", "reset-failed", name].map(str::to_owned));
        Ok(())
    }
}

pub(super) fn plain(path: &Path) -> Result<String, String> {
    let text = path.to_string_lossy().into_owned();
    if text.is_empty() || text.chars().any(|character| character.is_whitespace() || "\"'\\%$;".contains(character)) {
        return Err(format!("{text} has a character a unit's command line cannot carry"));
    }
    Ok(text)
}

/// The search path a started unit runs with: the operator's local and profile bins, then the system's.
pub(super) fn search_path(host: &Host) -> Result<String, String> {
    Ok([host.home.join(".local/bin"), host.home.join(NIX_PROFILE_BIN), PathBuf::from(SYSTEM_BIN)].iter().map(|entry| plain(entry)).collect::<Result<Vec<_>, _>>()?.join(":"))
}
