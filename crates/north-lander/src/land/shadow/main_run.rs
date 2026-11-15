use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::slots::select::Config;
use north_base::verdict_cache::reuse_log;
use super::super::slots::unit_reuse;
use north_offload::check_offload::{detached_scope, launch_detached};
use super::{Settings, place};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const TIMEOUT_SECONDS: &str = "3600";
const SHORT: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Started(usize),
    Harvested(usize),
    Waiting,
}

fn probe(main: &str) -> String {
    format!("main-{}", &main[..main.len().min(SHORT)])
}

fn marker(logs: &Path, main: &str) -> PathBuf {
    logs.join(format!("{}.recorded", probe(main)))
}

fn handle_of(logs: &Path, main: &str) -> PathBuf {
    logs.join(format!("{}.handle", probe(main)))
}

fn log_of(logs: &Path, probe: &str, index: usize) -> PathBuf {
    logs.join(format!("{probe}.{index}.log"))
}

fn exit_of(logs: &Path, probe: &str, index: usize) -> PathBuf {
    logs.join(format!("{probe}.{index}.exit"))
}

pub fn pending(logs: &Path, main: &str) -> bool {
    !marker(logs, main).exists()
}

fn north(settings: &Settings, arguments: &[&str], worktree: &Path) -> Option<Command> {
    let (program, prefix) = settings.north.split_first()?;
    let mut command = Command::new(program);
    command.args(prefix).args(arguments).current_dir(worktree).stdin(Stdio::null());
    Some(command)
}

pub fn lines(worktree: &Path, main: &str) -> Vec<String> {
    Config::load(worktree, main).commands(worktree, EMPTY_TREE, main).unwrap_or_default().into_iter().filter(|line| unit_reuse::runs_nextest(line)).collect()
}

fn start(settings: &Settings, worktree: &Path, main: &str, logs: &Path) -> Step {
    let lines = lines(worktree, main);
    if lines.is_empty() {
        let _ = fs::write(marker(logs, main), "");
        return Step::Waiting;
    }
    let host = place(&settings.chooser).ok().filter(|host| !host.is_empty()).unwrap_or_else(|| settings.host.clone());
    let probe = probe(main);
    let _ = fs::create_dir_all(logs);
    let scope = detached_scope();
    let mut launched = 0;
    for (index, line) in lines.iter().enumerate() {
        let wrapped = unit_reuse::wrapped("north", &probe, true, line);
        let exit = exit_of(logs, &probe, index);
        let _ = fs::remove_file(&exit);
        let Some(command) = north(settings, &["capacity", "offload", "run", "--background", "--host", &host, "--base", main, "--head", main, "--timeout", TIMEOUT_SECONDS, "--", &wrapped], worktree) else { continue };
        let Ok(log) = File::create(log_of(logs, &probe, index)) else { continue };
        if launch_detached(&command, &log, &exit, scope) {
            launched += 1;
        } else {
            let _ = fs::write(&exit, "1\n");
        }
    }
    let _ = fs::write(handle_of(logs, main), format!("{} {host}\n", lines.len()));
    Step::Started(launched)
}

fn harvest(settings: &Settings, worktree: &Path, main: &str, logs: &Path, handle: &str) -> Step {
    let mut words = handle.split_whitespace();
    let count: usize = words.next().and_then(|count| count.parse().ok()).unwrap_or(0);
    let host = words.next().unwrap_or(&settings.host);
    let probe = probe(main);
    if !(0..count).all(|index| exit_of(logs, &probe, index).exists()) {
        return Step::Waiting;
    }
    for index in 0..count {
        reuse_log::harvest(&log_of(logs, &probe, index), &unit_reuse::directory());
    }
    if let Some(mut sync) = north(settings, &["capacity", "offload", "verdict", "sync", "--host", host], worktree) {
        let _ = sync.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = fs::write(marker(logs, main), "");
    Step::Harvested(count)
}

pub fn advance(settings: &Settings, worktree: &Path, main: &str, logs: &Path) -> Step {
    match fs::read_to_string(handle_of(logs, main)) {
        Ok(handle) => harvest(settings, worktree, main, logs, &handle),
        Err(_) => start(settings, worktree, main, logs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_returns_while_its_recording_run_is_still_queued() {
        let world = tempfile::tempdir().unwrap();
        let exit = world.path().join("run.exit");
        let mut queued = Command::new("sleep");
        queued.arg("5");
        let log = File::create(world.path().join("run.log")).unwrap();
        assert!(launch_detached(&queued, &log, &exit, false));
        assert!(!exit.exists());
    }
}
