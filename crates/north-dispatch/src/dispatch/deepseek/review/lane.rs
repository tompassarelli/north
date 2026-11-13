//! The lane a DeepSeek session worked in, its git, and the mechanical checks that run before any reviewer launches.

use super::super::{mechanical, fleets};
use crate::pacing::placement;
use crate::platform::process::run_command_within;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const GIT_WAIT: Duration = Duration::from_secs(60);

/// A lane and the machine it lives on: this host, or the build host a session placed there ran on, reached over ssh.
pub(super) struct Worktree {
    pub(super) path: PathBuf,
    pub(super) machine: Option<(String, String)>,
}

impl Worktree {
    pub(super) fn here(path: PathBuf) -> Self {
        Self { path, machine: None }
    }

    /// The command that runs `words` in this lane's directory where it lives.
    pub(super) fn command(&self, words: &[&str]) -> Result<Command, String> {
        let path = self.path.to_str().ok_or("the lane path is not UTF-8")?;
        let (program, rest) = words.split_first().ok_or("no command")?;
        let Some((_, ssh)) = &self.machine else {
            let mut command = Command::new(crate::fleet::runtime::binary(program));
            command.args(rest).current_dir(&self.path);
            return Ok(command);
        };
        let script = format!("cd {} && PATH=\"$HOME/.local/bin:$PATH\" {}", quote(path), words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
        let mut command = Command::new(crate::fleet::runtime::binary("ssh"));
        command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", ssh, "--", &script]);
        Ok(command)
    }

    pub(super) fn exists(&self) -> bool {
        match &self.machine {
            None => self.path.is_dir(),
            Some(_) => self.command(&["true"]).ok().and_then(|command| run_command_within(command, None, GIT_WAIT)).is_some_and(|output| output.status.success()),
        }
    }

    /// The `--cwd` a session launched from this host takes to work in this lane.
    pub(super) fn launch_cwd(&self) -> String {
        match &self.machine {
            Some((name, _)) => format!("{name}:{}", self.path.display()),
            None => self.path.to_string_lossy().into_owned(),
        }
    }
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

pub(super) fn git(lane: &Worktree, args: &[&str]) -> Result<String, String> {
    let mut words = vec!["git"];
    words.extend(args);
    let output = run_command_within(lane.command(&words)?, None, GIT_WAIT).ok_or_else(|| format!("git {} did not finish", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The lane a DeepSeek session worked in: `worktrees/NAME` beside the `main` checkout it was launched in, which the
/// opencode launcher makes on the machine the session ran on, else that checkout when it ran here.
pub(super) fn lane(session: &Value, name: &str) -> Worktree {
    let cwd = Path::new(session["cwd"].as_str().unwrap_or_default());
    let made = cwd.parent().map(|container| container.join("worktrees").join(name));
    match session["machine"].as_str() {
        Some(machine) => {
            let ssh = placement::config().map_or_else(|_| machine.to_owned(), |cfg| fleets::destination(&cfg, machine));
            Worktree { path: made.unwrap_or_else(|| cwd.to_owned()), machine: Some((machine.to_owned(), ssh)) }
        }
        None => Worktree::here(made.filter(|lane| lane.is_dir()).unwrap_or_else(|| cwd.to_owned())),
    }
}

pub(super) fn north(words: &[&str]) -> Result<String, String> {
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(program).args(words).output().map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() { Ok(text) } else { Err(format!("north {}: {} {}", words.first().copied().unwrap_or_default(), text, String::from_utf8_lossy(&output.stderr).trim())) }
}

/// The files at `base` that define the tests named `oracle`.
pub(super) fn oracle_files(lane: &Worktree, base: &str, oracle: &[String]) -> Vec<String> {
    oracle.iter().flat_map(|name| git(lane, &["grep", "-l", "-e", &format!("fn {name}"), base, "--", "*.rs"]).unwrap_or_default().lines().filter_map(|hit| hit.split_once(':').map(|(_, file)| file.to_owned())).collect::<Vec<_>>()).collect()
}

/// The first mechanical check the lane's head fails, run before any reviewer launches.
pub(super) fn mechanical_failure(lane: &Worktree, oracle: &[String]) -> Result<Option<mechanical::Failure>, String> {
    let base = git(lane, &["merge-base", "origin/main", "HEAD"])?;
    let diff = git(lane, &["diff", "-U0", &format!("{base}..HEAD")])?;
    let subject = git(lane, &["log", "-1", "--format=%s"])?;
    let main_subjects: Vec<String> = git(lane, &["log", "--format=%s", "-n", "2000", "origin/main"])?.lines().map(str::to_owned).collect();
    let oracle_files = oracle_files(lane, &base, oracle);
    Ok(mechanical::failing(&mechanical::Lane { diff: &diff, subject: &subject, main_subjects: &main_subjects, oracle_files: &oracle_files }))
}
