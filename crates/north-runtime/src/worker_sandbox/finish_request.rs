//! The acceptance check of a finish call that the outside broker runs for a session whose domain keeps the verdict store unwritable (south#118): the finish record is the verdict a session's last finish call got for its acceptance command (north#1233), which the watcher's escalation and the leaf close read instead of any report, so a worker never writes it nor names it. The request is the words `finish-check COMMAND HEAD`, asked from the lane the finish call ran in. The broker runs COMMAND there in a new worker domain, as a Done-when check runs (check_request.rs), with the same environment and its `PATH` behind the shim directories that hold `north`, relays its output, and records the verdict from the command's own exit code for the session whose scope the caller runs in; the session is never a word of the request, the lane is one of the session's own, never a main checkout or a lane another live session launched in (`admitted`), the command is bounded, and HEAD must be the lane's own tip or the commit the finish call would make on it, whose tree must still be the tree of the lane's working state when the command has run, since the record vouches for that tree. When the command joins the lane's run before queueing (the switch `finish_acceptance_in_prequeue` is on, the command and every check before queueing are portable, and the broker's own placement names a build host), the broker runs that run instead, `north land checks --accept COMMAND --base BASE --head HEAD --runner-host HOST`, outside the worker domain as it runs a session's safe-push, with every check sent through the offload to build hosts the broker's own placement names, never run by that process itself, and records from that run's verdict on the command, so a hand-in takes one build-host admission (`Granted::joining`, north#1036).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::platform::finish_record::{FinishRecords, Finished};

use super::lane_grant::{self, Host};
use super::peer::Peer;
use super::offload_request;
use super::request_checkout::{self, Checkout};
use super::{CHECKED, FLAG, check_request};

pub(super) const VERB: &str = "finish-check";
const COMMAND_LIMIT: usize = 8192;
const TURN: &str = "finish.lock";
const SHAPE: &str = "the broker runs one finish check and takes no other word: finish-check COMMAND HEAD, with a command of at most 8192 bytes";

/// What a finish call asks the broker to run: its acceptance command and the commit it checks (empty when it checked none).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Check {
    command: String,
    head: String,
}

/// The process that runs a finish check, and the asking session's turn, kept while that process runs.
#[derive(Debug)]
pub(super) struct Granted {
    pub session: String,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: Vec<(String, String)>,
    command: String,
    head: String,
    _turn: File,
}

/// The words that ask the broker to run `command` as the finish check of `head`.
pub(super) fn request_words(command: &str, head: &str) -> Vec<String> {
    [VERB, command, head].map(str::to_owned).to_vec()
}

/// The `finish-check` request in `arguments`: `None` for any other request, and why one in other words is refused.
pub(super) fn asked(arguments: &[String]) -> Option<Result<Check, String>> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let [VERB, rest @ ..] = words.as_slice() else { return None };
    let hex = |head: &str| head.is_empty() || (matches!(head.len(), 40 | 64) && head.bytes().all(|byte| byte.is_ascii_hexdigit()));
    Some(match rest {
        [command, head] if !command.trim().is_empty() && command.len() <= COMMAND_LIMIT && hex(head) => Ok(Check { command: (*command).to_owned(), head: (*head).to_owned() }),
        _ => Err(SHAPE.to_owned()),
    })
}

/// The process that runs `check` for the caller `peer`, asked from `checkout`, on a host whose broker runs with `runtime` and `homes`, or why it is refused: the caller runs in no fleet session's scope, the commit is neither the lane's tip nor a commit made directly on it, or the session has a check running.
pub(super) fn granted(check: Check, checkout: &Checkout, peer: &Peer, host: &Host, runtime: &Path, homes: &[(String, String)]) -> Result<Granted, String> {
    let (directory, record) = lane_grant::own_session(peer, host, "a finish check is run for a fleet session, and this caller runs in no session's scope")?;
    let session = record["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned).ok_or_else(|| format!("{} names no session", directory.display()))?;
    admitted(&session, &record, &directory, checkout, peer, host)?;
    let environment = offload_request::environment(&host.environment, runtime, homes, Some(checkout))?;
    if !check.head.is_empty() && !on_lane(&check.head, checkout, &environment) {
        return Err(format!("{} is no commit of the lane {}: {session} checks the lane's own tip or the commit its finish call would make on it", check.head, checkout.top.display()));
    }
    let environment = check_request::environment(&session, environment, &host.home)?;
    let arguments = [FLAG, CHECKED, "--", "sh", "-c", &check.command].map(str::to_owned).to_vec();
    Ok(Granted { _turn: check_request::turn_in(&directory, &session, TURN)?, session, arguments, directory: checkout.top.clone(), environment, command: check.command, head: check.head })
}

/// Why `checkout` is not a lane the session `name`, whose record is `record` in the fleet directory `directory`, may run a finish check in, if it is not. An ordinary session's lanes are those under `worktrees/` of the project of the directory it launched in, one it made after launch among them (the sandbox gives it the same reach), never the main checkout and never the lane another live fleet session launched in, one placed here by another host among them, by its recorded directory or by its name; a lane-only session's is its own lane alone, since the scratch directory that would say which lanes it made since is not a fact the broker holds. The record vouches only for the tree the command ran on (`Granted::record`), so a lane another session could make is no gain to a caller.
pub(super) fn admitted(name: &str, record: &serde_json::Value, directory: &Path, checkout: &Checkout, peer: &Peer, host: &Host) -> Result<(), String> {
    let own = lane_grant::own_lane(name, record)?;
    if north_base::platform::session_scope::lane_only_cgroup(peer.cgroup().unwrap_or_default()) {
        return if own.canonicalize().ok().as_deref() == Some(checkout.top.as_path()) { Ok(()) } else { Err(format!("{} is not the lane of {name} ({}): a lane-only session's finish check runs in its own lane only", checkout.top.display(), own.display())) };
    }
    let cwd = PathBuf::from(record["cwd"].as_str().unwrap_or_default());
    let canonical = |path: PathBuf| path.canonicalize().unwrap_or(path);
    let (project, _) = request_checkout::project(&host.code, &cwd).ok_or_else(|| format!("{name} launched in {}, which is in no project of this host", cwd.display()))?;
    if checkout.top.parent() != Some(project.join("worktrees").as_path()) {
        return Err(format!("{} is no lane of {name}'s project {} (PROJECT/worktrees/NAME): a finish check runs in a lane, never in main", checkout.top.display(), project.display()));
    }
    let local = std::fs::read_dir(&host.fleet).into_iter().flatten().flatten().map(|entry| entry.path()).filter_map(|other| {
        let alive = north_base::platform::session_host_files::pid(&other).is_some_and(|pid| Path::new("/proc").join(pid.to_string()).exists());
        let other_record = std::fs::read(other.join("session.json")).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()).filter(|_| alive)?;
        Some((other, other_record))
    });
    for (_, other_record) in local.chain(lane_grant::placed_running(host)).filter(|(other, _)| other != directory) {
        let named = other_record["name"].as_str().unwrap_or_default();
        let launched = other_record["cwd"].as_str().and_then(|cwd| request_checkout::project(&host.code, Path::new(cwd))?.1);
        let theirs = launched.into_iter().chain(lane_grant::own_lane(named, &other_record).ok());
        if theirs.map(canonical).any(|their| their == checkout.top) {
            return Err(format!("{} is the lane live session {named} launched in, not one of {name}'s", checkout.top.display()));
        }
    }
    Ok(())
}

/// Whether `head` is the tip of the lane `checkout` or a commit whose first parent is that tip, read from the repository the broker placed the lane in.
fn on_lane(head: &str, checkout: &Checkout, environment: &[(String, String)]) -> bool {
    let git = |arguments: &[&str]| {
        let output = Command::new("git").arg("-C").arg(&checkout.top).args(arguments).env_clear().envs(environment.iter().cloned()).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success());
        output.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let (Some(tip), Some(parents)) = (git(&["rev-parse", "--verify", "HEAD"]), git(&["rev-list", "--parents", "-n", "1", head])) else { return false };
    let mut words = parents.split_whitespace();
    let commit = words.next().unwrap_or_default();
    commit == tip || (commit.eq_ignore_ascii_case(head) && words.next() == Some(tip.as_str()))
}

impl Granted {
    /// The same grant with the acceptance command run as one more check of the lane's run before queueing on the commit it names (north#1036), when the broker's own `north capacity offload choose` names a build host other than this one and the runtime's `north land checks` says the command joins that run (the switch is on, and the command and every check before queueing are portable): the broker starts that run itself, outside the worker domain as it starts a session's safe-push, so the land checks key and record each verdict as the lander's own, every check sent through the offload grant, first to that host (`--runner-host`) and spread over the hosts the same placement names, which may hold this host's own offload slots, never to one the head's `.safe-push` names and never run outside the offload, since the command's text is the worker's; and the exit code it records from is the run's verdict on the command, never a word of the request. The base is the destination's tip fetched as the finish call fetches it, so the keys are the ones safe-push reads. A grant that names no commit, a host with no build host to give, or a command that does not join keeps its own command, run in a worker domain.
    pub(super) fn joining(mut self, north: &Path) -> Self {
        if self.head.is_empty() {
            return self;
        }
        let Some(base) = self.base() else { return self };
        let north_says = |arguments: &[&str]| Command::new(north).args(arguments).current_dir(&self.directory).env_clear().envs(self.environment.iter().cloned()).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
        if north_says(&["land", "checks", "--accept", &self.command, "--joins", "--base", &base, "--head", &self.head]).is_none() {
            return self;
        }
        let here = crate::platform::host::name();
        let Some(host) = north_says(&["capacity", "offload", "choose"]).filter(|host| !host.is_empty() && *host != here && !host.contains(char::is_whitespace)) else { return self };
        self.arguments = ["land", "checks", "--accept", &self.command, "--base", &base, "--head", &self.head, "--runner-host", &host].map(str::to_owned).to_vec();
        self
    }

    fn base(&self) -> Option<String> {
        let git = |arguments: &[&str]| {
            let output = Command::new("git").arg("-C").arg(&self.directory).args(arguments).env_clear().envs(self.environment.iter().cloned()).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success());
            output.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|text| !text.is_empty())
        };
        let destination = git(&["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]).and_then(|reference| reference.strip_prefix("refs/remotes/origin/").map(str::to_owned)).filter(|name| !name.is_empty()).unwrap_or_else(|| "main".to_owned());
        let _ = git(&["fetch", "-q", "origin", &format!("+refs/heads/{destination}:refs/remotes/origin/{destination}")]);
        git(&["rev-parse", "--verify", &format!("refs/remotes/origin/{destination}^{{commit}}")])
    }

    /// Writes the session's finish record for the command's exit code `code`, in the store of the broker's own environment, once the lane's working state after the run is exactly the tree of the commit the record names: the command ran on that working state, and the record vouches for that tree, so a commit whose tree differs from what ran is recorded for nothing.
    pub(super) fn record(&self, code: u8, environment: &[(String, String)], home: &Path) -> Result<(), String> {
        if !self.head.is_empty() && !self.ran_on_head_tree()? {
            return Err(format!("the lane {} is not, after the run, the tree of {}, so no finish record is written for {}", self.directory.display(), self.head, self.session));
        }
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let finished = Finished { session: self.session.clone(), command: self.command.trim().to_owned(), head: self.head.clone(), passed: code == 0, at };
        FinishRecords::of(environment, home).record(&finished).map_err(|error| format!("the finish record of {}: {error}", self.session))
    }

    /// Whether the tree of the lane's working state now, HEAD's with every change and new file not ignored, is the tree of the commit the record names.
    fn ran_on_head_tree(&self) -> Result<bool, String> {
        let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
        let index = scratch.path().join("index");
        let git = |arguments: &[&str], own_index: bool| -> Result<String, String> {
            let mut command = Command::new("git");
            command.arg("-C").arg(&self.directory).args(["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false"]).args(arguments).env_clear().envs(self.environment.iter().cloned()).stdin(Stdio::null());
            if own_index {
                command.env("GIT_INDEX_FILE", &index);
            }
            let output = command.output().map_err(|error| format!("git: {error}"))?;
            if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()) } else { Err(format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())) }
        };
        git(&["read-tree", "HEAD"], true)?;
        git(&["add", "--all"], true)?;
        let working = git(&["write-tree"], true)?;
        let committed = git(&["rev-parse", "--verify", &format!("{}^{{tree}}", self.head)], false)?;
        Ok(working == committed)
    }
}

#[cfg(test)]
#[path = "finish_request_tests.rs"]
pub(super) mod tests;
