//! What the outside broker starts for a worker command's `capacity offload` request, which runs as the operator outside the worker's domain with the keys and master sockets that domain hides: nothing of the worker's but the typed words of the request (south#118). Its environment is the broker's own, never the client's, so the `PATH` that finds every program it starts is the broker's and no variable the client set claims an owner or a slot class; ssh and git are the absolute programs the broker resolved from that `PATH`; git runs no hook and no command a repository's configuration names; its working directory is the top of the lane or main checkout the client asked from, and the repository its git reads is the one the project's main checkout keeps for that checkout, named in its environment by the broker, so no `.git` the client can rewrite decides it; and its run ends by the longest deadline a land check has.

use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use north_base::foundation::offload::{GIT_ENV, HOLD_ENV, LANDER_SLOT_ENV, OWNER_ENV, RED_FIX_ENV, RUN_SECONDS_CEILING, SSH_ENV, SUBJECT_ENV};
use north_base::platform::git_credential;
use north_base::platform::session_scope::REMOVAL_ROOTS;

use super::lane_grant::GIT_HARDENING;
use super::request_checkout::{Checkout, PLACES};
use super::{CONFINED, HOSTS_HIDDEN, LANE_ONLY, MARK, SESSION_SCOPE, SOCKETS_HIDDEN};

const RUNTIME_ENV: &str = "XDG_RUNTIME_DIR";
const WITHHELD: [&str; 15] = [MARK, LANE_ONLY, SOCKETS_HIDDEN, CONFINED, HOSTS_HIDDEN, REMOVAL_ROOTS, SESSION_SCOPE, SUBJECT_ENV, OWNER_ENV, RED_FIX_ENV, LANDER_SLOT_ENV, HOLD_ENV, SSH_ENV, GIT_ENV, "PATH"];

/// The process the broker starts for one offload request.
pub(super) struct Offload {
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: Vec<(String, String)>,
    pub note: Option<String>,
}

impl Offload {
    /// Starts the process as the command `command` makes of the request's words, with the request's standard streams, telling the caller on its standard error when its run was shortened.
    pub(super) fn start(self, command: impl FnOnce(Vec<String>) -> Command, [stdin, stdout, stderr]: [OwnedFd; 3]) -> std::io::Result<Child> {
        if let (Some(note), Ok(told)) = (&self.note, stderr.try_clone()) {
            let _ = writeln!(File::from(told), "north worker-sandbox broker: {note}");
        }
        command(self.arguments)
            .current_dir(&self.directory)
            .env_clear()
            .envs(self.environment)
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .process_group(0)
            .spawn()
    }
}

/// Why an offload request is refused, if it is: a lane-only session sends none, and the host it names is one the asker's `reaches` list holds, never the caller's word alone, since these verbs are the one way a worker command runs anything on another host.
pub(super) fn offload_refusal(arguments: &[String], lane_only: bool, reached: impl Fn(&str) -> Result<String, String>) -> Option<String> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let ["capacity", "offload", rest @ ..] = words.as_slice() else { return None };
    if lane_only {
        return Some("a lane-only session's command reaches no build host through the broker".to_owned());
    }
    match crate::land_offload::worker_request(rest) {
        Some(Some(host)) => reached(host).err(),
        _ => None,
    }
}

/// The process that serves the offload request `arguments` asked from `checkout`, on a host whose broker runs with `own`, `runtime` and `homes`; `None` for any other request, and why an offload request is refused. It starts at the top of `checkout`, the one place the offload reads a repository from.
pub(super) fn granted(arguments: &[String], checkout: &Checkout, own: &[(String, String)], runtime: &Path, homes: &[(String, String)]) -> Result<Option<Offload>, String> {
    if !matches!(arguments, [noun, verb, ..] if noun == "capacity" && verb == "offload") {
        return Ok(None);
    }
    let (arguments, asked) = capped(arguments);
    let note = asked.map(|asked| format!("a worker command's run on a build host ends after {RUN_SECONDS_CEILING} s, the longest a land check runs, so --timeout {asked} is taken as {RUN_SECONDS_CEILING}"));
    Ok(Some(Offload { arguments, directory: checkout.top.clone(), environment: environment(own, runtime, homes, Some(checkout))?, note }))
}

/// `arguments` with a `run`'s `--timeout` no longer than the ceiling, and the seconds asked when they were more.
fn capped(arguments: &[String]) -> (Vec<String>, Option<u64>) {
    let mut capped = arguments.to_vec();
    let asked = arguments.len().checked_sub(3).filter(|at| *at >= 1 && arguments[at - 1] == "--timeout" && arguments[at + 1] == "--").and_then(|at| Some((at, arguments[at].parse::<u64>().ok()?))).filter(|(_, seconds)| *seconds > RUN_SECONDS_CEILING);
    if let Some((at, _)) = asked {
        capped[at] = RUN_SECONDS_CEILING.to_string();
    }
    (capped, asked.map(|(_, seconds)| seconds))
}

/// The environment of the offload process: `own`, the broker's, without the sandbox's markers, its git configuration and any owner or slot-class claim, then the absolute entries of the broker's `PATH`, `runtime`, `homes`, the ssh and git that `PATH` finds, the mark of a worker command's request, under which a `choose` claims no host, git's hooks, fsmonitor and ssh command off, and the repository of `checkout` as the one every git it runs reads (the offload runs git in that checkout alone); a process whose git runs in more than one checkout names none. Refused when the broker's `PATH` finds no ssh or no git.
pub(super) fn environment(own: &[(String, String)], runtime: &Path, homes: &[(String, String)], checkout: Option<&Checkout>) -> Result<Vec<(String, String)>, String> {
    let named = |name: &str| name == RUNTIME_ENV || WITHHELD.contains(&name) || PLACES.contains(&name) || name.starts_with("GIT_CONFIG") || homes.iter().any(|(home, _)| home == name);
    let path: Vec<PathBuf> = own.iter().find(|(name, _)| name == "PATH").map(|(_, value)| std::env::split_paths(value).filter(|entry| entry.is_absolute()).collect()).unwrap_or_default();
    let resolved = |program: &str| path.iter().map(|entry| entry.join(program)).find(|found| found.metadata().is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)).map(|found| found.to_string_lossy().into_owned()).ok_or_else(|| format!("the broker's own PATH finds no {program}, so no offload starts"));
    let programs = [(SSH_ENV.to_owned(), resolved("ssh")?), (GIT_ENV.to_owned(), resolved("git")?), (crate::land_offload::WORKER_REQUEST_ENV.to_owned(), "1".to_owned())];
    let path = std::env::join_paths(path.iter()).map_err(|error| error.to_string())?.to_string_lossy().into_owned();
    let kept = own.iter().filter(|(name, _)| !named(name)).cloned();
    Ok(kept.chain([("PATH".to_owned(), path), (RUNTIME_ENV.to_owned(), runtime.to_string_lossy().into_owned())]).chain(homes.iter().cloned()).chain(programs).chain(checkout.into_iter().flat_map(Checkout::pinned)).chain(git_credential::session_env_with(&GIT_HARDENING)).collect())
}
