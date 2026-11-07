use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::Gc;
use super::exec::{git, real};
use super::super::live_process::{any_inside, open_paths};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum BuildTarget {
    Absent,
    InUse,
    Dropped(u64),
    Failed(String),
}

pub(super) fn walk(path: &Path, delete: bool) -> io::Result<u64> {
    let meta = fs::symlink_metadata(path)?;
    let mut bytes = meta.blocks() * 512 / meta.nlink().max(1);
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            bytes += walk(&entry?.path(), delete)?;
        }
        if delete {
            fs::remove_dir(path)?;
        }
    } else if delete {
        fs::remove_file(path)?;
    }
    Ok(bytes)
}

pub(super) fn drop_build_target(lane: &Path, live: &[PathBuf], proc_root: &Path, dry_run: bool) -> BuildTarget {
    let target = real(lane).join("target");
    let is_dir = fs::symlink_metadata(&target).is_ok_and(|meta| meta.is_dir());
    if !is_dir || !git(lane, &["check-ignore", "-q", "target/"]).ok {
        return BuildTarget::Absent;
    }
    if any_inside(live, &target) || any_inside(&open_paths(proc_root), &target) {
        return BuildTarget::InUse;
    }
    match walk(&target, !dry_run) {
        Ok(bytes) => BuildTarget::Dropped(bytes),
        Err(error) => BuildTarget::Failed(error.to_string()),
    }
}

pub(super) fn gib(bytes: u64) -> String {
    format!("{:.1}G", bytes as f64 / f64::from(1u32 << 30))
}

impl Gc {
    pub(super) fn drop_target(&mut self, container: &str, lane: &Path, live: &[PathBuf], basis: &str) {
        let place = self.tilde(&lane.join("target"));
        match drop_build_target(lane, live, Path::new("/proc"), self.options.dry_run) {
            BuildTarget::Absent => {}
            BuildTarget::InUse => {
                if self.options.dry_run {
                    println!("{container}: keep build target {place}: open by a process");
                }
            }
            BuildTarget::Dropped(bytes) => {
                self.counts.dropped_targets += 1;
                self.counts.dropped_bytes += bytes;
                println!("{container}: {} build target {place} ({}, {basis})", if self.options.dry_run { "drop" } else { "dropped" }, gib(bytes));
            }
            BuildTarget::Failed(error) => println!("{container}: kept build target {place}: {error}"),
        }
    }
}
