use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::held::Held;
use super::{Inputs, Policy, Removal, hours, idle_for, removed, size};
use crate::platform::shared_target;

const BUILD_OUTPUT_IDLE: Duration = Duration::from_secs(4 * 3600);
const MAIN_BUILT_CONTAINER: &str = "north";
const PRIVATE_TARGET_PREFIX: &str = "cargo-target-";

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let limit = policy.idle.min(BUILD_OUTPUT_IDLE);
    let mut removals = Vec::new();
    for container in containers(&inputs.code) {
        let with_main = container.file_name().is_some_and(|name| name == MAIN_BUILT_CONTAINER);
        for checkout in checkouts(&container, with_main) {
            if let Some(path) = rebuildable(&checkout)
                && let Some(removal) = idle_unheld(inputs, policy.dry_run, limit, held, path, |idle| {
                    format!("cargo target of {} idle {}h, held by no process; the next build restores it", checkout.display(), hours(idle))
                })
            {
                removals.push(removal);
            }
        }
    }
    for path in private_targets(&inputs.cache) {
        if let Some(removal) = idle_unheld(inputs, policy.dry_run, limit, held, path, |idle| format!("lane-private cargo target idle {}h, held by no process", hours(idle))) {
            removals.push(removal);
        }
    }
    for path in mutant_runs(&inputs.cache) {
        if let Some(removal) = idle_unheld(inputs, policy.dry_run, limit, held, path, |idle| format!("mutant checkout left by a landing run idle {}h, held by no process; the next mutant run builds its own", hours(idle))) {
            removals.push(removal);
        }
    }
    if !policy.dry_run {
        for (target, freed) in shared_target::sweep_cache(&inputs.cache, inputs.now, shared_target::cap_bytes(&inputs.disk_budget))? {
            removals.push(Removal { path: target, reason: "shared cargo target over its cap: idle incremental state, test binaries, libraries and profiles".to_owned(), bytes: freed });
        }
        for (target, freed) in shared_target::sweep_mutant(&inputs.cache, inputs.now, shared_target::mutant_cap_bytes(&inputs.disk_budget))? {
            removals.push(Removal { path: target, reason: "mutant cargo target over its cap: idle incremental state, test binaries, libraries and profiles".to_owned(), bytes: freed });
        }
    }
    Ok(removals)
}

fn idle_unheld(inputs: &Inputs, dry_run: bool, limit: Duration, held: &Held, path: PathBuf, reason: impl FnOnce(Duration) -> String) -> Option<Removal> {
    if held.within(&path) {
        return None;
    }
    let idle = idle_for(&path, inputs.now).ok().filter(|idle| *idle >= limit)?;
    let bytes = size(&path);
    if !dry_run && !removed(&path) {
        return None;
    }
    Some(Removal { reason: reason(idle), path, bytes })
}

fn checkouts(container: &Path, with_main: bool) -> Vec<PathBuf> {
    let mut checkouts = if with_main { vec![container.join("main")] } else { Vec::new() };
    if let Ok(worktrees) = fs::read_dir(container.join("worktrees")) {
        checkouts.extend(worktrees.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path()));
    }
    checkouts
}

fn rebuildable(checkout: &Path) -> Option<PathBuf> {
    let target = checkout.join("target");
    let real_dir = fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.is_dir());
    (real_dir && target.join("CACHEDIR.TAG").is_file()).then_some(target)
}

fn private_targets(cache: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(cache) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(PRIVATE_TARGET_PREFIX) && entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

fn containers(code: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(code) else { return Vec::new() };
    let mut containers: Vec<PathBuf> = entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path()).collect();
    containers.sort();
    containers
}

fn mutant_runs(cache: &Path) -> Vec<PathBuf> {
    let Ok(repos) = fs::read_dir(cache.join("north/mutant")) else { return Vec::new() };
    repos.flatten().filter_map(|repo| fs::read_dir(repo.path()).ok()).flat_map(|runs| runs.flatten()).filter(|run| run.file_type().is_ok_and(|kind| kind.is_dir())).map(|run| run.path()).collect()
}
