use super::failure::{DEFERRED, Failure, code, failed};
use super::layout::{self, Layout};
use crate::capacity::game_reserve::Confinement;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::SystemTime;

const STALE_INCREMENTAL_LINK: &str = "undefined hidden symbol";
const KEPT_LOGS: usize = 8;

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

    pub(super) fn links_against_stale_incremental_output(&self) -> bool {
        fs::read_to_string(&self.path).is_ok_and(|output| output.contains(STALE_INCREMENTAL_LINK))
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

pub(super) fn forget_release_incremental(built: &Path) -> io::Result<()> {
    match fs::remove_dir_all(built.join("release/incremental")) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

pub(super) fn cargo_words(confined: Option<&Confinement>) -> Vec<String> {
    let cargo = ["nix", "develop", "-c", "cargo", "build", "--release", "--locked", "--timings"].map(str::to_owned);
    match confined {
        Some(confinement) => ["nice", "-n", "19", "ionice", "-c", "3"].map(str::to_owned).into_iter().chain(cargo).chain(["-j".to_owned(), confinement.jobs.to_string()]).collect(),
        None => cargo.to_vec(),
    }
}

pub(super) fn cargo_ran(built: &Path, since: SystemTime) -> bool {
    fs::metadata(built.join("cargo-timings/cargo-timing.html")).and_then(|metadata| metadata.modified()).is_ok_and(|written| written >= since)
}

pub(super) fn build_failure(target: &str, status: ExitStatus, cargo_ran: bool, log: &BuildLog) -> Failure {
    match cargo_ran {
        true => failed(code(status), format!("build of {target} failed: {status}; build log: {}", log.path().display())),
        false => failed(DEFERRED, format!("capacity admission or its scope stopped the build of {} before cargo ran ({status}); retrying on the next tick; build log: {}", layout::short(target), log.path().display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_keeps_room_for_the_next_log_beside_the_newest_ones() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..5 {
            let log = directory.path().join(format!("{index}.log"));
            fs::write(&log, "").unwrap();
            File::options().write(true).open(&log).unwrap().set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(index)).unwrap();
        }
        prune(directory.path(), 3).unwrap();
        let mut left: Vec<String> = fs::read_dir(directory.path()).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["3.log", "4.log"]);
    }

    #[test]
    fn a_confined_build_runs_cargo_at_idle_priority_with_jobs_capped_at_the_agent_cpus() {
        let words = cargo_words(Some(&Confinement { cpus: "4-7".to_owned(), jobs: 4 }));
        assert_eq!(words[..6], ["nice", "-n", "19", "ionice", "-c", "3"]);
        assert_eq!(words[words.len() - 2..], ["-j", "4"]);
        assert!(!cargo_words(None).iter().any(|word| word == "nice" || word == "-j"));
    }
}
