use std::path::{Path, PathBuf};
use std::process::Command;

use super::candidate::{self, git};
use super::queue::{self, Ticket};

pub const DEPTH: usize = 2;
const SSH_OPTIONS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"];

pub struct Lander {
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub spare: PathBuf,
    pub queue: PathBuf,
    pub checker: Vec<String>,
    pub app_token: Vec<String>,
    pub depth: usize,
}

pub(super) fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
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
    pub fn new(repo: &Path, checker: Vec<String>) -> Result<Self, String> {
        let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
        Ok(Self {
            repo: repo.to_owned(),
            worktree: candidate::lander_worktree(repo, "lander")?,
            spare: candidate::lander_worktree(repo, "lander-2")?,
            queue: queue::directory(&state_home(), &origin),
            checker,
            app_token: vec![super::credential::MINT.to_owned()],
            depth: DEPTH,
        })
    }

    fn remote_hosts(&self) -> Vec<String> {
        git(&self.repo, &["config", "--blob", "refs/remotes/origin/main:.safe-push", "--get", "landing.remote-hosts"]).unwrap_or_default().split_whitespace().map(str::to_owned).collect()
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
