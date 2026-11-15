use north_runtime_records::failure::{DEFERRED, Failure, code, failed};
use super::layout::{self, Layout};
use crate::capacity::game_reserve::Confinement;
use north_build_system::{Build, Profile, Resolution, Shell};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

const STALE_INCREMENTAL_LINK: &str = "undefined hidden symbol";
const ARCHIVE: &str = " in archive ";
const KEPT_LOGS: usize = 8;
const IDLE: [&str; 6] = ["nice", "-n", "19", "ionice", "-c", "3"];

pub(super) struct BuildLog {
    path: PathBuf,
}

impl BuildLog {
    pub(super) fn open(layout: &Layout, target: &str) -> io::Result<Self> {
        let directory = layout.build_logs();
        fs::create_dir_all(&directory)?;
        prune(&directory, KEPT_LOGS)?;
        Ok(Self { path: directory.join(format!("{target}.log")) })
    }

    pub(super) fn attach(&self, command: &mut Command) -> io::Result<()> {
        let log = File::create(&self.path)?;
        command.stdout(log.try_clone()?).stderr(log);
        Ok(())
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn stale_archives(&self) -> Option<Vec<PathBuf>> {
        let output = fs::read_to_string(&self.path).ok().filter(|output| output.contains(STALE_INCREMENTAL_LINK))?;
        let mut archives: Vec<PathBuf> = output.lines().filter_map(|line| line.rsplit_once(ARCHIVE)).map(|(_, archive)| PathBuf::from(archive.trim())).filter(|archive| archive.extension().is_some_and(|extension| extension == "rlib")).collect();
        archives.sort();
        archives.dedup();
        Some(archives)
    }

    pub(super) fn discard(self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn prune(directory: &Path, kept: usize) -> io::Result<()> {
    let mut logs: Vec<(SystemTime, PathBuf)> = fs::read_dir(directory)?
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "log"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    logs.sort_by_key(|log| std::cmp::Reverse(log.0));
    for (_, stale) in logs.into_iter().skip(kept.saturating_sub(1)) {
        fs::remove_file(stale)?;
    }
    Ok(())
}

pub(super) fn recompile_from_scratch(built: &Path, archives: &[PathBuf]) -> io::Result<()> {
    forget_incremental_of(built, archives)?;
    for archive in archives.iter().filter(|archive| archive.starts_with(built)) {
        match fs::remove_file(archive) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// Only the crates whose archives failed to link lose their incremental output, so the retry recompiles those and re-checks their dependents incrementally; clearing every crate's output made it a build from scratch of everything downstream.
fn forget_incremental_of(built: &Path, archives: &[PathBuf]) -> io::Result<()> {
    let crates: Vec<String> = archives.iter().filter_map(|archive| archive.file_stem()?.to_str()?.strip_prefix("lib")?.rsplit_once('-').map(|(name, _)| format!("{name}-"))).collect();
    let incremental = built.join("release/incremental");
    let entries = match fs::read_dir(&incremental) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        entries => entries?,
    };
    for entry in entries {
        let entry = entry?;
        if crates.iter().any(|name| entry.file_name().to_string_lossy().starts_with(name.as_str())) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// The release build of every runtime program of `lane` into `built`, held to the confinement's jobs when the host is in game-reserve.
pub(super) fn release_build<'a>(lane: &'a Path, built: &'a Path, confined: Option<&Confinement>, uncached: bool) -> Build<'a> {
    Build { root: lane, profile: Profile::Release, binary: None, programs: &north_base::platform::store_refs::RUNTIME_PROGRAMS, jobs: confined.map(|confinement| confinement.jobs as usize), resolution: Resolution::LOCKED, shell: Shell::NixDevelop, output: Some(built), uncached, timings: true }
}

/// The words that run a confined build at idle CPU and I/O priority, none for a build that is not confined.
pub(super) fn idle(confined: Option<&Confinement>) -> impl Iterator<Item = &'static str> {
    confined.map(|_| IDLE).into_iter().flatten()
}

pub(super) fn build_failure(target: &str, status: ExitStatus, cargo_ran: bool, log: &BuildLog) -> Failure {
    match cargo_ran {
        true => failed(code(status), format!("build of {target} failed: {status}; build log: {}", log.path().display())),
        false => failed(DEFERRED, format!("capacity admission or its scope stopped the build of {} before cargo ran ({status}); retrying on the next tick; build log: {}", layout::short(target), log.path().display())),
    }
}
