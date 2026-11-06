use std::path::{Path, PathBuf};
use std::process::Command;

use super::candidate::{self, git};
use super::heartbeat::Heartbeat;
use super::jobs::Chores;
use super::queue::{self, Ticket};
use super::setting;

pub const DEPTH: usize = 2;
pub const OFFLOAD_OWNER: &str = "north-land";
const SSH_OPTIONS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"];

pub struct Lander {
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub spares: Vec<PathBuf>,
    pub queue: PathBuf,
    pub checker: Vec<String>,
    pub app_token: Vec<String>,
    pub offload_owner: String,
    pub round: std::sync::atomic::AtomicU64,
    pub chores: Chores,
    pub heartbeat: Heartbeat,
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
}

pub fn offload_owner(queue: &Path) -> String {
    format!("{OFFLOAD_OWNER}-{}", queue.file_name().and_then(|name| name.to_str()).unwrap_or_default())
}

fn plain(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('-') && text.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@'))
}

fn ssh(host: &str, script: &str) -> Result<String, String> {
    let output = Command::new("ssh").args(SSH_OPTIONS).args(["-T", host, "--", script]).output().map_err(|error| format!("ssh {host}: {error}"))?;
    if !output.status.success() {
        return Err(format!("ssh {host} exited {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Lander {
    pub fn new(repo: &Path, checker: Vec<String>, depth: usize) -> Result<Self, String> {
        let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
        let queue = queue::directory(&state_home(), &origin);
        Ok(Self {
            repo: repo.to_owned(),
            worktree: candidate::lander_worktree(repo, "lander")?,
            spares: (2..=depth).map(|level| candidate::lander_worktree(repo, &format!("lander-{level}"))).collect::<Result<_, _>>()?,
            offload_owner: offload_owner(&queue),
            queue,
            checker,
            app_token: super::credential::mint_program(),
            round: std::sync::atomic::AtomicU64::new(0),
            chores: Chores::default(),
            heartbeat: Heartbeat::default(),
        })
    }

    pub fn detached(&self) -> Self {
        Self {
            repo: self.repo.clone(),
            worktree: self.worktree.clone(),
            spares: self.spares.clone(),
            queue: self.queue.clone(),
            checker: self.checker.clone(),
            app_token: self.app_token.clone(),
            offload_owner: self.offload_owner.clone(),
            round: std::sync::atomic::AtomicU64::new(self.round.load(std::sync::atomic::Ordering::Relaxed)),
            chores: self.chores,
            heartbeat: Heartbeat::default(),
        }
    }

    fn remote_hosts(&self) -> Vec<String> {
        setting::landing(&self.repo, "refs/remotes/origin/main", "landing.remote-hosts").split_whitespace().map(str::to_owned).collect()
    }

    fn fetch_lane(&self, host: &str, ticket: &Ticket) -> Result<(), String> {
        let url = format!("ssh://{host}{}", ticket.root.display());
        let target = format!("+refs/heads/{}:refs/land-remote/{host}/{}", ticket.branch, queue::slug(&ticket.branch));
        let ssh_command = std::iter::once("ssh").chain(SSH_OPTIONS.iter().copied()).collect::<Vec<_>>().join(" ");
        let output = Command::new("git").arg("-C").arg(&self.repo).args(["fetch", "-q", "--no-tags", &url, &target]).env("GIT_SSH_COMMAND", ssh_command).output().map_err(|error| format!("git fetch {url}: {error}"))?;
        if !output.status.success() {
            return Err(format!("git fetch {url} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(())
    }

    fn pull_host(&self, host: &str) -> Result<usize, String> {
        let key = self.queue.file_name().and_then(|name| name.to_str()).ok_or_else(|| format!("{} has no queue key", self.queue.display()))?;
        let outbox = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/land-queue/{key}/{}\"", queue::OUTBOX);
        let listing = ssh(host, &format!("d={outbox}; [ -d \"$d\" ] || exit 0; for f in \"$d\"/*.json; do [ -f \"$f\" ] || continue; printf '%s\\n' \"${{f##*/}}\"; cat \"$f\"; printf '\\000'; done"))?;
        let mut taken = Vec::new();
        let mut failures = Vec::new();
        for record in listing.split('\0').filter(|record| !record.trim().is_empty()) {
            let (name, json) = record.split_once('\n').unwrap_or((record, ""));
            if !plain(name) || !name.ends_with(".json") {
                failures.push(format!("{name:?} is not a ticket name"));
                continue;
            }
            let ticket: Ticket = match serde_json::from_str(json) {
                Ok(ticket) => ticket,
                Err(error) => {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
            };
            if let Err(error) = self.fetch_lane(host, &ticket) {
                failures.push(error);
                continue;
            }
            let path = self.queue.join(name);
            queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            taken.push(name.to_owned());
        }
        if !taken.is_empty() {
            ssh(host, &format!("d={outbox}; mkdir -p \"$d/taken\" && cd \"$d\" && mv -f -- {} taken/", taken.join(" ")))?;
        }
        if failures.is_empty() { Ok(taken.len()) } else { Err(failures.join("; ")) }
    }

    pub fn pull_remote(&self) -> Vec<String> {
        let mut waiting = Vec::new();
        for host in self.remote_hosts() {
            if !plain(&host) {
                waiting.push(format!("landing.remote-hosts names {host:?}, which is not an ssh host name; skipped"));
                continue;
            }
            match self.pull_host(&host) {
                Ok(0) => {}
                Ok(pulled) => println!("north land: pulled {pulled} tickets from {host}'s outbox"),
                Err(error) => waiting.push(format!("{host}'s outbox skipped this round: {error}")),
            }
        }
        waiting
    }
}
