//! `north depbox check -- COMMAND`: runs a land check's command with a scratch directory of its own as TMPDIR and a record of the environment it started with, so a program the check runs through the capacity gate keeps, inside depbox, the variables the check set for it and that scratch directory, while the environment the check inherited stays behind the fixed carry list.
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

pub use super::carry::{CHECK_CARRY as CARRY, CHECK_START as START};
pub const SCRATCH: &str = "TMPDIR";
const SCRATCH_PREFIX: &str = "north-check-";
const SCRATCH_PARENT: &str = "/tmp";
const CONTROL_PREFIX: &str = "DEPBOX";

pub fn run(command: &[String]) -> u8 {
    let Some((program, rest)) = command.split_first() else {
        eprintln!("usage: north depbox check -- <command> [args...]");
        return 2;
    };
    let scratch = match tempfile::Builder::new().prefix(SCRATCH_PREFIX).tempdir_in(SCRATCH_PARENT) {
        Ok(scratch) => scratch,
        Err(error) => {
            eprintln!("depbox check: scratch directory under {SCRATCH_PARENT}: {error}");
            return 1;
        }
    };
    let environment = std::env::vars_os().filter(|(name, _)| name != SCRATCH).chain([(SCRATCH.into(), scratch.path().as_os_str().to_owned())]);
    let status = Command::new(program).args(rest).env(SCRATCH, scratch.path()).env(START, record(environment)).status();
    let code = match status {
        Ok(status) => status.code().map_or_else(|| 128 + status.signal().unwrap_or(0) as u8, |code| code as u8),
        Err(error) => {
            eprintln!("depbox check: {program}: {error}");
            127
        }
    };
    if code != 0 {
        let _ = scratch.keep();
    }
    code
}

/// The names and value digests of `environment`, one `NAME\tDIGEST` line each.
pub fn record(environment: impl IntoIterator<Item = (OsString, OsString)>) -> String {
    environment.into_iter().filter_map(|(name, value)| Some(format!("{}\t{}\n", name.to_str()?, digest(&value)))).collect()
}

/// The variables a program the gate sends into depbox carries in, given the check's start `record`: each one the check set or changed since it started, and TMPDIR only while it still names the check's own scratch directory; never depbox's own control variables.
pub fn carried(environment: impl IntoIterator<Item = (OsString, OsString)>, record: &str) -> Vec<String> {
    let started: std::collections::HashSet<&str> = record.lines().collect();
    let mut names: Vec<String> = environment
        .into_iter()
        .filter_map(|(name, value)| {
            let name = name.into_string().ok()?;
            let unchanged = started.contains(format!("{name}\t{}", digest(&value)).as_str());
            let keep = if name == SCRATCH { unchanged } else { !unchanged && carryable(&name) };
            keep.then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// Whether a check may carry the variable `name` into depbox: any but depbox's own control variables, which decide its lease and its sandbox.
pub fn carryable(name: &str) -> bool {
    !name.starts_with(CONTROL_PREFIX)
}

/// The check's scratch directory at `path`, held open so depbox binds the very directory it checked: one of this user's directly under /tmp with the scratch prefix, opened without following a link.
pub fn scratch(path: &Path) -> Option<OwnedFd> {
    use rustix::fs::{Mode, OFlags};
    let named = path.parent() == Some(Path::new(SCRATCH_PARENT)) && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(SCRATCH_PREFIX));
    if !named {
        return None;
    }
    let directory = rustix::fs::open(path, OFlags::PATH | OFlags::NOFOLLOW | OFlags::DIRECTORY, Mode::empty()).ok()?;
    let owned = rustix::fs::fstat(&directory).is_ok_and(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Directory && stat.st_uid == rustix::process::getuid().as_raw());
    owned.then_some(directory)
}

fn digest(value: &OsString) -> String {
    Sha256::digest(value.as_bytes())[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "depbox_check_tests.rs"]
mod tests;
