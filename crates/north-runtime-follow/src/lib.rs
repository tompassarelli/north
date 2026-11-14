//! How a long-lived north process moves onto the runtime its link promotes (docs/rationale/runtime-follow.md): it asks
//! `replaced` at each of its safe points and, once the link names another runtime, re-execs that runtime in place or
//! exits so its unit starts it. It also holds the tree layout those runtimes live in, the runtime this process runs
//! from, and the collection that deletes the ones no link, lease, live process or agent link needs, so a crate that
//! only places or builds a runtime can read them without depending on a whole product line.
#![forbid(unsafe_code)]

pub mod collection;
pub mod layout;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The variable a unit sets to follow a link other than `current`, as the lander's units name the lander link.
pub const LINK: &str = "NORTH_RUNTIME_LINK";
/// The file beside the lander link that names the runtime a rollback of the link would go back to while a promotion judges a newer one, so runtime collection keeps it.
pub const LANDER_ROLLBACK: &str = "lander-rollback";
/// How long a busy process may finish what it holds after its link moved, so it moves within a minute of the flip.
pub const DEFER: Duration = Duration::from_secs(45);
const DELETED_SUFFIX: &[u8] = b" (deleted)";

/// The runtime a process started on and the link naming the one it should run.
pub struct Follow {
    link: PathBuf,
    started_on: Option<String>,
    program: Option<PathBuf>,
    noticed: Option<Instant>,
}

impl Follow {
    /// This process against `NORTH_RUNTIME_LINK`, else the host's `current` link.
    pub fn current() -> Self {
        Self::of(link(), std::env::current_exe().ok())
    }

    /// A host service a client starts on demand: it serves every client of the host, so it follows the host's `current`
    /// link whatever link its starter followed, the lander's or a canary's judged one.
    pub fn service() -> Self {
        Self::of(host_link(), std::env::current_exe().ok())
    }

    /// A process running `executable` against `link`: a promoted executable started on its own runtime, any other on
    /// the runtime the link named when it started.
    pub fn of(link: PathBuf, executable: Option<PathBuf>) -> Self {
        let promoted = executable.map(without_deleted_suffix).and_then(|executable| runtime_of(&executable));
        let started_on = promoted.as_ref().map(|(name, _)| name.clone()).or_else(|| named(&link));
        Self { link, started_on, program: promoted.map(|(_, program)| program), noticed: None }
    }

    pub fn link(&self) -> &Path {
        &self.link
    }

    /// The runtime the link names when it is not the one this process runs.
    pub fn replaced(&mut self) -> Option<String> {
        let replacement = named(&self.link).filter(|named| Some(named) != self.started_on.as_ref());
        match replacement {
            Some(_) => _ = self.noticed.get_or_insert_with(Instant::now),
            None => self.noticed = None,
        }
        replacement
    }

    /// Whether a process that defers while busy must move now: its runtime was replaced and it is idle or has
    /// deferred for `DEFER`.
    pub fn due(&mut self, idle: bool) -> bool {
        self.replaced().is_some() && (idle || self.noticed.is_some_and(|noticed| noticed.elapsed() >= DEFER))
    }

    /// Replaces this process image with the same program of the link's runtime and the same arguments, keeping its
    /// pid, its children and every descriptor not marked close-on-exec; returns only the error that stopped it.
    pub fn reexec(&self) -> io::Error {
        self.reexec_with(std::env::args_os().skip(1))
    }

    /// The program this process would run under the link's runtime.
    pub fn next(&self) -> io::Result<PathBuf> {
        let program = self.program.as_ref().ok_or_else(|| io::Error::other("this process runs no promoted runtime, so it has no build to move to"))?;
        let runtime = fs::canonicalize(&self.link).map_err(|error| io::Error::other(format!("{}: {error}", self.link.display())))?;
        Ok(runtime.join(program))
    }

    /// `reexec` with `arguments` in place of this process's own.
    pub fn reexec_with(&self, arguments: impl IntoIterator<Item = OsString>) -> io::Error {
        let next = match self.next() {
            Ok(next) => next,
            Err(error) => return error,
        };
        Command::new(&next).arg0(&next).args(arguments).exec()
    }
}

/// `NORTH_RUNTIME_LINK` when set, else the host's `current` link.
pub fn link() -> PathBuf {
    std::env::var_os(LINK).filter(|value| !value.is_empty()).map_or_else(host_link, PathBuf::from)
}

/// The host's `current` link, `$XDG_STATE_HOME/north/current`, whatever `NORTH_RUNTIME_LINK` names.
pub fn host_link() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    state.join("north/current")
}

/// The runtime a link names: the last component of its target.
pub fn named(link: &Path) -> Option<String> {
    fs::read_link(link).ok()?.file_name()?.to_str().map(str::to_owned)
}

/// The runtime directory an executable lies under (`runtimes/SHA/`) and its path inside it.
pub fn runtime_of(executable: &Path) -> Option<(String, PathBuf)> {
    executable.ancestors().find_map(|directory| {
        let name = directory.file_name()?.to_str()?;
        if !is_sha(name) || directory.parent()?.file_name()? != "runtimes" {
            return None;
        }
        Some((name.to_owned(), executable.strip_prefix(directory).ok()?.to_path_buf()))
    })
}

fn is_sha(name: &str) -> bool {
    name.len() == 40 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// The runtime this process runs from: its executable's `runtimes/SHA` directory, when the directory carries every
/// program a runtime holds.
pub fn own_runtime() -> Option<(String, std::path::PathBuf)> {
    let executable = fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    let directory = executable.parent()?;
    let sha = directory.file_name()?.to_str()?;
    (layout::is_sha(sha) && layout::has_programs(directory)).then(|| (sha.to_owned(), directory.to_path_buf()))
}

/// Linux reports /proc/self/exe with a " (deleted)" suffix once the running binary has been replaced on disk.
fn without_deleted_suffix(path: PathBuf) -> PathBuf {
    match path.as_os_str().as_bytes().strip_suffix(DELETED_SUFFIX) {
        Some(stripped) => PathBuf::from(OsString::from_vec(stripped.to_vec())),
        None => path,
    }
}
