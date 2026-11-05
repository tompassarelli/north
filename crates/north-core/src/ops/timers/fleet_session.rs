use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::{child_failure, north_program, text};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Placement {
    Account(String),
    Held(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum LaunchError {
    Held(String),
    Failed(String),
}

pub(super) struct Launch<'a> {
    pub name: &'a str,
    pub account: &'a str,
    pub cwd: &'a Path,
    pub brief: &'a str,
    pub model: Option<&'a str>,
}

pub(super) fn held(reason: &str) -> bool {
    ["exhausted", "saturated", "no Claude account", "held:"].iter().any(|word| reason.contains(word))
}

pub(super) fn placement(output: &str) -> Option<Placement> {
    let target = output.lines().find_map(|line| line.split_once(": target ").map(|(_, rest)| rest).or_else(|| line.strip_prefix("target ")))?;
    let first = target.split_whitespace().next().unwrap_or_default();
    let account = target.split_whitespace().nth(1).and_then(|word| word.strip_prefix("claude:"));
    Some(match account {
        Some(account) if !first.ends_with(':') && first != "anthropic-cloud" => Placement::Account(account.to_owned()),
        _ => Placement::Held(target.to_owned()),
    })
}

pub(super) fn run(command: &mut Command, name: &str) -> Result<Output, String> {
    let output = command.output().map_err(|error| format!("{name}: {error}"))?;
    if output.status.success() { Ok(output) } else { Err(child_failure(name, output.status.code(), &output.stderr)) }
}

pub(super) fn git(directory: &Path, args: &[&str]) -> Result<String, String> {
    run(Command::new("git").arg("-C").arg(directory).args(args), "git").map(|output| text(&output).trim().to_owned())
}

pub(super) struct Fleet {
    pub south: PathBuf,
}

impl Fleet {
    pub fn sessions(&self) -> Result<Vec<String>, String> {
        let listed = run(Command::new(north_program()).args(["fleet", "list"]), "north fleet list")?;
        Ok(text(&listed).lines().map(str::to_owned).collect())
    }

    pub fn place(&self, brief: &str) -> Result<Placement, String> {
        let file = tempfile::Builder::new().prefix("timer-brief").suffix(".md").tempfile().map_err(|error| error.to_string())?;
        fs::write(file.path(), brief).map_err(|error| error.to_string())?;
        let output = Command::new(north_program()).args(["agents", "plan", "--place"]).arg(file.path()).output().map_err(|error| format!("north agents plan --place: {error}"))?;
        let combined = format!("{}\n{}", text(&output), String::from_utf8_lossy(&output.stderr));
        placement(&combined).ok_or_else(|| child_failure("north agents plan --place", output.status.code(), &output.stderr))
    }

    pub fn worktree(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.south.parent().unwrap_or(&self.south).join("worktrees").join(name);
        git(&self.south, &["fetch", "--quiet", "origin"])?;
        git(&self.south, &["worktree", "add", "--quiet", "-b", name, &path.to_string_lossy(), "origin/main"])?;
        Ok(path)
    }

    pub fn discard(&self, name: &str, path: &Path) {
        let _ = git(&self.south, &["worktree", "remove", "--force", &path.to_string_lossy()]);
        let _ = git(&self.south, &["branch", "-D", name]);
    }

    pub fn launch(&self, launch: &Launch) -> Result<(), LaunchError> {
        let mut command = Command::new(north_program());
        command.args(["fleet", "launch", "--provider", "claude", "--account", launch.account, "--parent", "none", "--name", launch.name]);
        if let Some(model) = launch.model {
            command.args(["--model", model]);
        }
        let output = command.arg("--cwd").arg(launch.cwd).arg(launch.brief).output().map_err(|error| LaunchError::Failed(format!("north fleet launch: {error}")))?;
        if output.status.success() {
            return Ok(());
        }
        let reason = child_failure("north fleet launch", output.status.code(), &output.stderr);
        Err(if held(&reason) { LaunchError::Held(reason) } else { LaunchError::Failed(reason) })
    }

    pub fn tell_proxy(&self, text: &str) -> Result<(), String> {
        run(Command::new(north_program()).args(["fleet", "send", "--from", "service:north-fleet-session", "proxy"]).arg(text), "north fleet send proxy").map(drop)
    }
}
