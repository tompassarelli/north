//! What the outside broker starts for a worker command's `capacity offload` request, which runs as the operator outside the worker's domain with the keys and master sockets that domain hides: nothing of the worker's but the typed words of the request (south#118). Its environment is the broker's own, never the client's, so the `PATH` that finds every program it starts is the broker's and no variable the client set claims an owner or a slot class; ssh and git are the absolute programs the broker resolved from that `PATH`; git runs no hook and no command a repository's configuration names; its working directory is the top of the lane or main checkout the client asked from, under the broker's own home; and its run ends by the longest deadline a land check has.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use north_base::foundation::offload::{GIT_ENV, LANDER_SLOT_ENV, OWNER_ENV, RED_FIX_ENV, RUN_SECONDS_CEILING, SSH_ENV};
use north_base::platform::git_credential;
use north_base::platform::session_scope::REMOVAL_ROOTS;

use super::lane_grant::GIT_HARDENING;
use super::removable::{lane, project};
use super::{HOSTS_HIDDEN, LANE_ONLY, MARK, SESSION_SCOPE, SOCKETS_HIDDEN};

const RUNTIME_ENV: &str = "XDG_RUNTIME_DIR";
const WITHHELD: [&str; 12] = [MARK, LANE_ONLY, SOCKETS_HIDDEN, HOSTS_HIDDEN, REMOVAL_ROOTS, SESSION_SCOPE, OWNER_ENV, RED_FIX_ENV, LANDER_SLOT_ENV, SSH_ENV, GIT_ENV, "PATH"];

/// The process the broker starts for one offload request.
pub(super) struct Offload {
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: Vec<(String, String)>,
    pub note: Option<String>,
}

/// The process that serves the offload request `arguments` asked from `directory`, on a host whose home is `home` and whose broker runs with `own`, `runtime` and `homes`; `None` for any other request, and why an offload request is refused.
pub(super) fn granted(arguments: &[String], directory: &Path, home: &Path, own: &[(String, String)], runtime: &Path, homes: &[(String, String)]) -> Result<Option<Offload>, String> {
    if !matches!(arguments, [noun, verb, ..] if noun == "capacity" && verb == "offload") {
        return Ok(None);
    }
    let directory = checkout(home, directory)?;
    let (arguments, asked) = capped(arguments);
    let note = asked.map(|asked| format!("a worker command's run on a build host ends after {RUN_SECONDS_CEILING} s, the longest a land check runs, so --timeout {asked} is taken as {RUN_SECONDS_CEILING}"));
    Ok(Some(Offload { arguments, directory, environment: environment(own, runtime, homes)?, note }))
}

/// The top of the lane (`~/code/PROJECT/worktrees/NAME`) or main checkout (`~/code/PROJECT/main`) under `home` that `directory` lies in: the one place the offload reads a repository from, since git run anywhere else would read a repository, and its configuration, of the client's making.
fn checkout(home: &Path, directory: &Path) -> Result<PathBuf, String> {
    let found = home.canonicalize().ok().zip(directory.canonicalize().ok()).and_then(|(home, real)| {
        let main = project(&home, &real).map(|project| project.join("main")).filter(|main| real.starts_with(main));
        lane(&home, &real).or(main)
    });
    found.filter(|top| top.join(".git").exists()).ok_or_else(|| format!("{} is in no lane (~/code/PROJECT/worktrees/NAME) and no main checkout (~/code/PROJECT/main) of this host, and an offload ships only from one; run it from your lane", directory.display()))
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

/// The environment of the offload process: `own`, the broker's, without the sandbox's markers, its git configuration and any owner or slot-class claim, then the absolute entries of the broker's `PATH`, `runtime`, `homes`, the ssh and git that `PATH` finds, and git's hooks, fsmonitor and ssh command off. Refused when the broker's `PATH` finds no ssh or no git.
fn environment(own: &[(String, String)], runtime: &Path, homes: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let named = |name: &str| name == RUNTIME_ENV || WITHHELD.contains(&name) || name.starts_with("GIT_CONFIG") || homes.iter().any(|(home, _)| home == name);
    let path: Vec<PathBuf> = own.iter().find(|(name, _)| name == "PATH").map(|(_, value)| std::env::split_paths(value).filter(|entry| entry.is_absolute()).collect()).unwrap_or_default();
    let resolved = |program: &str| path.iter().map(|entry| entry.join(program)).find(|found| found.metadata().is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)).map(|found| found.to_string_lossy().into_owned()).ok_or_else(|| format!("the broker's own PATH finds no {program}, so no offload starts"));
    let programs = [(SSH_ENV.to_owned(), resolved("ssh")?), (GIT_ENV.to_owned(), resolved("git")?)];
    let path = std::env::join_paths(path.iter()).map_err(|error| error.to_string())?.to_string_lossy().into_owned();
    let kept = own.iter().filter(|(name, _)| !named(name)).cloned();
    Ok(kept.chain([("PATH".to_owned(), path), (RUNTIME_ENV.to_owned(), runtime.to_string_lossy().into_owned())]).chain(homes.iter().cloned()).chain(programs).chain(git_credential::session_env_with(&GIT_HARDENING)).collect())
}
