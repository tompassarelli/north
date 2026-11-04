use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

pub const SCRUBBED: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN", "SOPS_AGE_KEY", "SOPS_AGE_KEY_FILE", "SOPS_AGE_SSH_PRIVATE_KEY_FILE"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Refused(String),
    RedMain(String),
    Transient(String),
    Deferred(String),
}

pub fn parse(text: &str) -> Option<Verdict> {
    let line = text.lines().rev().find_map(|line| line.trim_end().strip_prefix("safe-push: verdict "))?;
    let (class, detail) = line.split_once(' ').unwrap_or((line, ""));
    let detail = detail.trim().to_owned();
    Some(match class {
        "green" => Verdict::Green,
        "refused" => Verdict::Refused(detail),
        "red-main" => Verdict::RedMain(detail),
        "transient" => Verdict::Transient(detail),
        "deferred" => Verdict::Deferred(detail),
        other => Verdict::Transient(format!("unknown verdict class {other}")),
    })
}

pub struct Started {
    child: Child,
    program: String,
    log: PathBuf,
}

pub fn start(checker: &[String], worktree: &Path, base: &str, log: &Path, own_group: bool) -> Result<Started, Verdict> {
    let Some((program, prefix)) = checker.split_first() else {
        return Err(Verdict::Transient("no checker configured".to_owned()));
    };
    if let Some(parent) = log.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Err(Verdict::Transient(format!("{}: {error}", parent.display())));
    }
    let (stdout, stderr) = match File::create(log).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok(files) => files,
        Err(error) => return Err(Verdict::Transient(format!("{}: {error}", log.display()))),
    };
    let mut command = Command::new(program);
    command.args(prefix).args(["--check-candidate", "--base", base]).current_dir(worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env("NORTH_LAND_SERVING", "1");
    if own_group {
        command.process_group(0);
    }
    for name in SCRUBBED {
        command.env_remove(name);
    }
    match command.spawn() {
        Ok(child) => Ok(Started { child, program: program.clone(), log: log.to_owned() }),
        Err(error) => Err(Verdict::Transient(format!("{program}: {error}"))),
    }
}

impl Started {
    pub fn poll(&mut self) -> Option<Verdict> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(self.verdict(status)),
            Ok(None) => None,
            Err(error) => Some(Verdict::Transient(format!("{}: {error}", self.program))),
        }
    }

    pub fn wait(mut self) -> Verdict {
        match self.child.wait() {
            Ok(status) => self.verdict(status),
            Err(error) => Verdict::Transient(format!("{}: {error}", self.program)),
        }
    }

    pub fn kill(mut self) {
        if let Some(pid) = rustix::process::Pid::from_raw(self.child.id().cast_signed()) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Kill);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn verdict(&self, status: ExitStatus) -> Verdict {
        let (program, log) = (&self.program, &self.log);
        let text = fs::read_to_string(log).unwrap_or_default();
        match parse(&text) {
            Some(Verdict::Refused(_) | Verdict::Green) if status.code() == Some(75) => Verdict::Transient(format!("{program} exited 75; log: {}", log.display())),
            Some(Verdict::Refused(detail)) if detail.is_empty() => Verdict::Refused(log.display().to_string()),
            Some(verdict) => verdict,
            None => Verdict::Transient(format!("{program} gave no verdict line ({status}); log: {}", log.display())),
        }
    }
}

pub fn run(checker: &[String], worktree: &Path, base: &str, log: &Path) -> Verdict {
    match start(checker, worktree, base, log, false) {
        Ok(started) => started.wait(),
        Err(verdict) => verdict,
    }
}
