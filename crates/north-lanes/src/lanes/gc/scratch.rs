use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::Gc;
use super::targets::{BuildTarget, gib, walk};
use super::super::live_process::any_inside;

const CARGO_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.";

fn is_cargo_target(dir: &Path) -> bool {
    fs::read_to_string(dir.join("CACHEDIR.TAG")).is_ok_and(|tag| tag.starts_with(CARGO_TAG))
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(dir).into_iter().flatten().flatten();
    entries.filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path()).collect()
}

fn last_build(target: &Path) -> SystemTime {
    let profiles = subdirs(target);
    let outputs = profiles.iter().flat_map(|profile| subdirs(profile));
    std::iter::once(target.to_path_buf())
        .chain(profiles.iter().cloned())
        .chain(outputs)
        .filter_map(|dir| fs::metadata(dir).and_then(|meta| meta.modified()).ok())
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

pub(super) fn scratch_targets(scratch: &Path) -> Vec<PathBuf> {
    let mut targets: Vec<PathBuf> =
        subdirs(scratch).into_iter().flat_map(|dir| [dir.join("target"), dir]).filter(|dir| fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir()) && is_cargo_target(dir)).collect();
    targets.sort();
    targets
}

pub(super) fn drop_scratch_target(target: &Path, open: &[PathBuf], now: SystemTime, idle: Duration, dry_run: bool) -> BuildTarget {
    if any_inside(open, target) || now.duration_since(last_build(target)).unwrap_or_default() < idle {
        return BuildTarget::InUse;
    }
    match walk(target, !dry_run) {
        Ok(bytes) => BuildTarget::Dropped(bytes),
        Err(error) => BuildTarget::Failed(error.to_string()),
    }
}

impl Gc {
    pub(super) fn drop_scratch_targets(&mut self, scratch: &Path, open: &[PathBuf]) {
        let idle = Duration::from_secs_f64(self.options.idle_hours.max(0.0) * 3600.0);
        for target in scratch_targets(scratch) {
            match drop_scratch_target(&target, open, SystemTime::now(), idle, self.options.dry_run) {
                BuildTarget::Dropped(bytes) => {
                    self.counts.dropped_targets += 1;
                    self.counts.dropped_bytes += bytes;
                    println!("scratch: {} cargo target {} ({}, no build within {}h)", if self.options.dry_run { "drop" } else { "dropped" }, target.display(), gib(bytes), self.options.idle_hours);
                }
                BuildTarget::Failed(error) => println!("scratch: kept cargo target {}: {error}", target.display()),
                BuildTarget::Absent | BuildTarget::InUse => {}
            }
        }
    }
}
