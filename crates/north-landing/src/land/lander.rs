use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::candidate::{self, git};
use super::heartbeat::Heartbeat;
use super::jobs::Chores;
use super::queue::{self, Ticket};
use super::setting;
use north_base::land_ticket::Records;
use north_base::land_ticket::Phase;

pub const OFFLOAD_OWNER: &str = "north-land";
const SSH_OPTIONS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"];
const WAIT_TICK: Duration = Duration::from_millis(200);

pub struct Lander {
    pub repo: PathBuf,
    pub worktree: PathBuf,
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

pub fn plain(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('-') && text.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '@'))
}

fn ssh(host: &str, script: &str) -> Result<String, String> {
    ssh_with_input(host, script, b"")
}

fn ssh_with_input(host: &str, script: &str, input: &[u8]) -> Result<String, String> {
    let mut child = Command::new("ssh").args(SSH_OPTIONS).args(["-T", host, "--", script]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("ssh {host}: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    let output = child.wait_with_output().map_err(|error| format!("ssh {host}: {error}"))?;
    if !output.status.success() {
        return Err(format!("ssh {host} exited {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Lander {
    pub fn base_moved(&self, base: &str) -> Result<bool, String> {
        let now = git(&self.worktree, &["ls-remote", "origin", "refs/heads/main"])?;
        Ok(now.split_whitespace().next() != Some(base))
    }

    pub fn new(repo: &Path, checker: Vec<String>) -> Result<Self, String> {
        let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
        let queue = queue::directory(&state_home(), &origin);
        Ok(Self {
            repo: repo.to_owned(),
            worktree: candidate::lander_worktree(repo, "lander")?,
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
            queue: self.queue.clone(),
            checker: self.checker.clone(),
            app_token: self.app_token.clone(),
            offload_owner: self.offload_owner.clone(),
            round: std::sync::atomic::AtomicU64::new(self.round.load(std::sync::atomic::Ordering::Relaxed)),
            chores: self.chores,
            heartbeat: Heartbeat::default(),
        }
    }

    pub fn remote_hosts(&self) -> Vec<String> {
        setting::outbox_hosts(&self.repo, "refs/remotes/origin/main")
    }

    fn fetch_lane(&self, host: &str, ticket: &Ticket) -> Result<(), String> {
        north_git_locks::retry_after_clearing(&self.repo, || self.fetch_lane_once(host, ticket))
    }

    fn fetch_lane_once(&self, host: &str, ticket: &Ticket) -> Result<(), String> {
        let url = format!("ssh://{host}{}", ticket.root.display());
        let target = format!("+refs/heads/{}:refs/land-remote/{host}/{}", ticket.branch, queue::slug(&ticket.branch));
        let ssh_command = std::iter::once("ssh").chain(SSH_OPTIONS.iter().copied()).collect::<Vec<_>>().join(" ");
        let output = Command::new("git").arg("-C").arg(&self.repo).args(["fetch", "-q", "--no-tags", &url, &target]).env("GIT_SSH_COMMAND", ssh_command).output().map_err(|error| format!("git fetch {url}: {error}"))?;
        if !output.status.success() {
            return Err(format!("git fetch {url} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(())
    }

    pub fn pull_host(&self, host: &str, wait: Duration) -> Result<usize, String> {
        let key = self.queue.file_name().and_then(|name| name.to_str()).ok_or_else(|| format!("{} has no queue key", self.queue.display()))?;
        let home = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/land-queue/{key}\"");
        let outbox = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/land-queue/{key}/{}\"", queue::OUTBOX);
        let mirror = format!("q={home}; mkdir -p \"$q\" && cat >\"$q/{0}.part\" && mv -f \"$q/{0}.part\" \"$q/{0}\" || exit 1; ", queue::LANDER_LEDGER);
        let ticks = wait.as_millis() / WAIT_TICK.as_millis();
        let listing = ssh_with_input(host, &format!("d={outbox}; n={ticks}; while [ \"$n\" -gt 0 ]; do for f in \"$d\"/*.json; do [ -f \"$f\" ] && break 2; done; n=$((n-1)); sleep {tick}; done; set -- \"$d\"/*.json; [ -e \"$1\" ] || exit 0; {mirror} for f in \"$d\"/*.json; do [ -f \"$f\" ] || continue; printf '%s\\n' \"${{f##*/}}\"; cat \"$f\"; printf '\\000'; done", tick = WAIT_TICK.as_secs_f32()), ledger_tail().as_bytes())?;
        let mut records = Records::open(&self.queue).map_err(|error| format!("cannot read the records of {}: {error}", self.queue.display()))?;
        let mut taken = Vec::new();
        let mut failures = Vec::new();
        let mut withdrawn = Vec::new();
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
                if lane_gone(host, &ticket) {
                    withdrawn.push((name.to_owned(), ticket));
                } else {
                    failures.push(error);
                }
                continue;
            }
            let path = self.queue.join(name);
            if records.phase(&path).is_none() {
                queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                records.moved(&path, Phase::Queued, &format!("taken from {host}'s outbox")).map_err(|error| format!("cannot record {}: {error}", path.display()))?;
            }
            taken.push(name.to_owned());
        }
        if !taken.is_empty() {
            ssh(host, &format!("d={outbox}; mkdir -p \"$d/taken\" && cd \"$d\" && mv -f -- {} taken/", taken.join(" ")))?;
        }
        if !withdrawn.is_empty() {
            self.withdraw(host, &outbox, withdrawn)?;
        }
        if failures.is_empty() { Ok(taken.len()) } else { Err(failures.join("; ")) }
    }

    fn withdraw(&self, host: &str, outbox: &str, withdrawn: Vec<(String, Ticket)>) -> Result<(), String> {
        let mut told = Vec::new();
        for (name, mut ticket) in withdrawn {
            let reason = format!("its worktree {} is gone from {host}, so the lander cannot fetch {}", ticket.root.display(), ticket.head);
            ticket.verdict = Some(format!("withdrawn by {OFFLOAD_OWNER}: {reason}"));
            let path = self.queue.join(queue::WITHDRAWN).join(&name);
            Records::open(&self.queue).and_then(|mut records| records.moved(&path, Phase::Withdrawn, ticket.verdict.as_deref().unwrap_or_default())).map_err(|error| format!("cannot record {}: {error}", path.display()))?;
            queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("north land: withdrew {name} from {host}'s outbox: {reason}");
            told.push((name, ticket, reason));
        }
        let names: Vec<&str> = told.iter().map(|(name, _, _)| name.as_str()).collect();
        ssh(host, &format!("d={outbox}; mkdir -p \"$d/{0}\" && cd \"$d\" && mv -f -- {1} {0}/", queue::WITHDRAWN, names.join(" ")))?;
        for (_, ticket, reason) in told {
            if let Some(session) = ticket.session.as_deref().filter(|session| !session.is_empty()) {
                super::after::send(session, &format!("north land: {} ({}) was withdrawn from {host}'s outbox: {reason}; queue it again from a worktree on its branch", ticket.branch, ticket.head));
            }
        }
        Ok(())
    }

    pub fn pull_remote(&self) -> Vec<String> {
        let mut waiting = Vec::new();
        for host in self.remote_hosts() {
            if !plain(&host) {
                waiting.push(format!("the outbox hosts name {host:?}, which is not an ssh host name; skipped"));
                continue;
            }
            match self.pull_host(&host, Duration::ZERO) {
                Ok(0) => {}
                Ok(pulled) => println!("north land: pulled {pulled} tickets from {host}'s outbox"),
                Err(error) => waiting.push(format!("{host}'s outbox skipped this round: {error}")),
            }
        }
        waiting
    }
}

fn ledger_tail() -> String {
    let rows = std::fs::read_to_string(super::post_land::ledger_path()).unwrap_or_default();
    let lines: Vec<&str> = rows.lines().collect();
    lines[lines.len().saturating_sub(500)..].iter().map(|line| format!("{line}\n")).collect()
}

fn lane_gone(host: &str, ticket: &Ticket) -> bool {
    let root = ticket.root.to_string_lossy().replace('\'', "'\\''");
    ssh(host, &format!("[ -e '{root}/.git' ] && echo present || echo gone")).is_ok_and(|answer| answer.trim() == "gone")
}
