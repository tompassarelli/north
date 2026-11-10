use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Gc;
use super::archive;
use super::exec::{LIMIT, git_with, real, verified};
use super::super::live_process::any_inside;
use super::super::reap::disposable;
use super::super::repo::is_lander_tree;
use north_base::platform::land_queue;

const HOUR_MS: f64 = 3_600_000.0;
const BUILD_OUTPUT: [&str; 2] = ["target", ".direnv"];

pub(super) struct Lanes<'a> {
    pub(super) main: &'a Path,
    pub(super) base: &'a str,
    pub(super) project: &'a str,
    pub(super) directory: &'a Path,
    pub(super) common: &'a Path,
    pub(super) registered: &'a [PathBuf],
    pub(super) live: &'a [PathBuf],
}

fn newest_ms(directory: &Path) -> f64 {
    let mut newest = 0.0_f64;
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&next) else { continue };
        let modified = meta.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map_or(0.0, |since| since.as_secs_f64() * 1000.0);
        newest = newest.max(modified);
        if meta.is_dir() {
            let entries = std::fs::read_dir(&next).into_iter().flatten().flatten();
            pending.extend(entries.filter(|entry| !BUILD_OUTPUT.contains(&entry.file_name().to_string_lossy().as_ref())).map(|entry| entry.path()));
        }
    }
    newest
}

fn strewn(lanes: &Lanes, directory: &Path) -> Vec<String> {
    let at = [("GIT_DIR", lanes.common), ("GIT_WORK_TREE", directory)];
    let listed = git_with(directory, &["ls-files", "--others", "--ignored", "--exclude-standard"], LIMIT, &at);
    listed.out.lines().filter(|path| !path.is_empty() && !disposable(path)).map(str::to_owned).collect()
}

impl Gc {
    pub(super) fn archive_unregistered(&mut self, lanes: &Lanes) {
        let mut found: Vec<PathBuf> = std::fs::read_dir(lanes.directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
        found.sort();
        for directory in found {
            let Some(name) = directory.file_name().map(|name| name.to_string_lossy().into_owned()) else { continue };
            let plain = std::fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir());
            if !plain || name.starts_with('.') || is_lander_tree(&directory) || lanes.registered.contains(&real(&directory)) {
                continue;
            }
            let label = self.tilde(&directory);
            let hold = self.unregistered_hold(lanes, &directory);
            if !hold.is_empty() {
                println!("{}: unregistered {label}: keep ({hold})", lanes.project);
                continue;
            }
            let tip = verified(lanes.main, &format!("refs/heads/{name}")).unwrap_or_default();
            let branch = if tip.is_empty() { "" } else { name.as_str() };
            self.archive_directory(lanes, &directory, branch, &tip, &label);
        }
    }

    fn archive_directory(&mut self, lanes: &Lanes, directory: &Path, branch: &str, tip: &str, label: &str) {
        let why = "git no longer registers this lane directory";
        let spec = archive::Lane { main: lanes.main, base: lanes.base, project: lanes.project, worktree: Some(directory), branch, tip, issues: &[], why };
        if self.options.dry_run {
            println!("{}: archive {label} ({why}) to {}, then reap", lanes.project, self.tilde(&self.options.archive.join(lanes.project)));
            return;
        }
        match archive::archive_unregistered(&spec, directory, lanes.common, &self.options.archive, self.now_ms) {
            Ok(file) => {
                println!("{}: archived {label} ({why}) to {}, then reaped", lanes.project, self.tilde(&file));
                self.counts.archived += 1;
                self.counts.retired_worktrees += 1;
            }
            Err(error) => println!("{}: kept {label}: {error}", lanes.project),
        }
    }

    fn unregistered_hold(&mut self, lanes: &Lanes, directory: &Path) -> String {
        if any_inside(lanes.live, &real(directory)) {
            return "process inside".into();
        }
        if (self.now_ms as f64 - newest_ms(directory)) / HOUR_MS < self.options.idle_hours {
            return format!("touched within {}h", self.options.idle_hours);
        }
        match self.session_in(directory) {
            Ok(true) => return "live Fleet session in lane".into(),
            Err(_) => return "Fleet unavailable".into(),
            Ok(false) => {}
        }
        if let Some(ticket) = land_queue::waiting_on(&land_queue::queues(&self.home.join(".local/state")), &land_queue::local_host(), directory).first() {
            return format!("land ticket {} still waits", ticket.display());
        }
        match strewn(lanes, directory).first() {
            Some(first) => format!("ignored files ({first})"),
            None => String::new(),
        }
    }
}
