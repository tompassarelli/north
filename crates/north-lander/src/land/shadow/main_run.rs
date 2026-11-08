use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::slots::select::Config;
use super::super::slots::unit_reuse::{self, harvest};
use super::{Settings, place};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const TIMEOUT_SECONDS: &str = "5400";
const SHORT: usize = 12;

fn marker(logs: &Path, main: &str) -> PathBuf {
    logs.join(format!("main-{}.recorded", &main[..main.len().min(SHORT)]))
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

pub fn record(settings: &Settings, worktree: &Path, main: &str, logs: &Path) -> Option<usize> {
    let lines = lines(worktree, main);
    if lines.is_empty() {
        let _ = fs::write(marker(logs, main), "");
        return None;
    }
    let host = place(&settings.chooser).ok().filter(|host| !host.is_empty()).unwrap_or_else(|| settings.host.clone());
    let probe = format!("main-{}", &main[..main.len().min(SHORT)]);
    let _ = fs::create_dir_all(logs);
    for (index, line) in lines.iter().enumerate() {
        let wrapped = unit_reuse::wrapped("north", &probe, true, line);
        let log = logs.join(format!("{probe}.{index}.log"));
        let Some(mut command) = north(settings, &["capacity", "offload", "run", "--background", "--host", &host, "--base", main, "--head", main, "--timeout", TIMEOUT_SECONDS, "--", &wrapped], worktree) else { continue };
        let Ok(output) = File::create(&log) else { continue };
        let Ok(errors) = output.try_clone() else { continue };
        let _ = command.stdout(output).stderr(errors).status();
        harvest::harvest(&log, &harvest::directory());
    }
    if let Some(mut sync) = north(settings, &["capacity", "offload", "verdict", "sync", "--host", &host], worktree) {
        let _ = sync.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = fs::write(marker(logs, main), "");
    Some(lines.len())
}
