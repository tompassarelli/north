//! The Done-when check the outside broker runs for a session whose domain keeps the ledger and the checker's lock unwritable (north#1137): a session asks for the check of one box of the issue it was launched for, one check at a time, and never writes its verdict. The request is the words `task check --done-when --box TEXT OWNER/REPO#N` and nothing else of the caller's: no directory, no environment and no option reach the process the broker starts. That process is the broker's own north as the checker, in the main checkout its home registers for the issue's repository, which no worker domain writes and whose origin it has read to be that repository, with the environment the broker gives an offload's process (offload_request.rs) and its `PATH` behind the gh shim under the asking session's name. The checker reads the command from the issue, runs it in a new worker domain on its own checkout of origin/main, so a command someone wrote into an issue has a session's reach and no more, and writes the ledger record, the Evidence stamp and the tick itself, outside that domain.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::fs::{FlockOperation, flock};

use super::lane_grant::{self, Host};
use super::offload_request;
use super::request_checkout;

const TURN: &str = "check.lock";
const SHAPE: &str = "the broker checks one box of one issue and takes no other word: north task check --done-when --box TEXT OWNER/REPO#N";

/// One box of one issue, named by its first words.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Check {
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub words: String,
}

/// The process that serves a check, and the asking session's turn, kept while that process runs.
#[derive(Debug)]
pub(super) struct Granted {
    pub session: String,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: Vec<(String, String)>,
    _turn: File,
}

/// The fleet session that asks: its name, the issue it was launched for and its directory among the fleet's records, which no session's domain writes.
struct Asking {
    name: String,
    task: String,
    directory: PathBuf,
}

/// The check `arguments` ask for: `None` for a request that is no `task check`, and why a `task check` in any other words is refused.
pub(super) fn asked(arguments: &[String]) -> Option<Result<Check, String>> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let ["task", "check", rest @ ..] = words.as_slice() else { return None };
    Some(match rest {
        ["--done-when", "--box", text, reference] if !text.trim().is_empty() => issue(reference).map(|(owner, repository, number)| Check { owner, repository, number, words: (*text).to_owned() }).ok_or_else(|| format!("{reference} is not OWNER/REPO#N; {SHAPE}")),
        _ => Err(SHAPE.to_owned()),
    })
}

fn issue(reference: &str) -> Option<(String, String, u64)> {
    let named = |name: &str| !name.is_empty() && !name.starts_with(['.', '-']) && name.chars().all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
    let (slug, number) = reference.split_once('#')?;
    let (owner, repository) = slug.split_once('/')?;
    (named(owner) && named(repository)).then(|| Some((owner.to_owned(), repository.to_owned(), number.parse().ok()?))).flatten()
}

/// The checker process for `check`, asked by a caller whose `/proc/PID/cgroup` reads `cgroup`, on a host whose broker runs with `runtime` and `homes`, or why it is refused: the caller runs in no fleet session's scope, the issue is not the one that session was launched for, the host keeps no main checkout of the issue's repository or holds no gh shim, or the session has a check running.
pub(super) fn granted(check: &Check, cgroup: &str, host: &Host, runtime: &Path, homes: &[(String, String)]) -> Result<Granted, String> {
    let asking = session(cgroup, host)?;
    let reference = format!("{}/{}#{}", check.owner, check.repository, check.number);
    if !lane_grant::same_issue(&reference, &asking.task) {
        return Err(format!("{} was launched for {}, and a session asks for the check of a box of its own task issue only, not of {reference}", asking.name, asking.task));
    }
    let environment = environment(&asking.name, offload_request::environment(&host.environment, runtime, homes, None)?, &host.home)?;
    let directory = main_checkout(check, &host.code, &environment)?;
    let arguments = ["task", "check", "--done-when", "--confined", "--box", &check.words, &reference].map(str::to_owned).to_vec();
    Ok(Granted { _turn: turn(&asking)?, session: asking.name, arguments, directory, environment })
}

/// `own`, the environment the broker gives an offload's process (offload_request.rs), with its `PATH` behind `home`'s gh shim, so the checker's tick reaches GitHub as the Agent App, under the asking session's `name`.
pub(super) fn environment(name: &str, own: Vec<(String, String)>, home: &Path) -> Result<Vec<(String, String)>, String> {
    let path = lane_grant::shimmed_path(home, own.iter().find(|(variable, _)| variable == "PATH").map_or("", |(_, value)| value.as_str()))?;
    Ok(own.into_iter().filter(|(variable, _)| variable != "PATH" && variable != lane_grant::SESSION_ENV).chain([("PATH".to_owned(), path), (lane_grant::SESSION_ENV.to_owned(), name.to_owned())]).collect())
}

/// The fleet session whose own unit `cgroup` names.
fn session(cgroup: &str, host: &Host) -> Result<Asking, String> {
    let (directory, record) = lane_grant::own_session(cgroup, host, "a check is asked from a fleet session's own scope, and this caller runs in none")?;
    let field = |key: &str| record[key].as_str().filter(|value| !value.is_empty()).map(str::to_owned);
    let name = field("name").ok_or_else(|| format!("{} names no session", directory.display()))?;
    let task = field("task_ref").ok_or_else(|| format!("{name} was launched with no --task, so it has no issue of its own whose box it could ask a check of"))?;
    Ok(Asking { name, task, directory })
}

/// The one check `asking` runs at a time: an exclusive lock on a file in its fleet directory, held until the returned file is dropped.
fn turn(asking: &Asking) -> Result<File, String> {
    turn_in(&asking.directory, &asking.name, TURN)
}

/// The exclusive lock on the file `lock` in the fleet directory `directory` of the session `name`, held until the returned file is dropped.
pub(super) fn turn_in(directory: &Path, name: &str, lock: &str) -> Result<File, String> {
    let path = directory.join(lock);
    let held = File::options().create(true).append(true).open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    flock(&held, FlockOperation::NonBlockingLockExclusive).map(|()| held).map_err(|_| format!("{name} has a check running, and a session runs one at a time; ask again when it ends"))
}

/// The main checkout of the repository `check` names among the projects under `code` (`CODE/REPO/main`), as the broker places one (request_checkout.rs), whose origin, read from the repository that checkout keeps, is that repository.
fn main_checkout(check: &Check, code: &[PathBuf], environment: &[(String, String)]) -> Result<PathBuf, String> {
    let slug = format!("{}/{}", check.owner, check.repository);
    let main = code.iter().find_map(|root| request_checkout::registered(code, &root.join(&check.repository).join("main")).ok()).ok_or_else(|| format!("this host keeps no main checkout of {slug}"))?;
    let read = Command::new("git").arg("-C").arg(&main.top).args(["config", "--get", "remote.origin.url"]).env_clear().envs(environment.iter().cloned()).envs(main.pinned()).output().map_err(|error| format!("git config in {}: {error}", main.top.display()))?;
    let origin = String::from_utf8_lossy(&read.stdout).trim().to_owned();
    if origin_names(&origin, &slug) { Ok(main.top) } else { Err(format!("{} has the origin {origin:?}, which is not {slug}", main.top.display())) }
}

/// Whether the remote address `origin` ends in the repository `slug` (`OWNER/REPO`), with or without `.git`.
fn origin_names(origin: &str, slug: &str) -> bool {
    let path = origin.strip_suffix(".git").unwrap_or(origin).to_ascii_lowercase();
    path.strip_suffix(&slug.to_ascii_lowercase()).is_some_and(|before| before.ends_with(['/', ':']))
}

#[cfg(test)]
#[path = "check_request_tests.rs"]
mod tests;
