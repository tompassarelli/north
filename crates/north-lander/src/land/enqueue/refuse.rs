use std::path::Path;

use super::facts::Repository;
use crate::land::candidate::{git, output};
use crate::land::lane_gates::{self, Lane};
use crate::land::scans::git::merge_base;
use crate::dep_gate::Registry;
use crate::landing::{merge_set, stale_revert};

pub(super) fn refuse_archived(repository: &Repository) -> Result<(), String> {
    if repository.archived {
        return Err(format!("GitHub reports {} as archived, so origin refuses every push; unarchive it or land elsewhere", repository.name));
    }
    Ok(())
}

pub(in crate::land) fn refuse_static_gates(root: &Path, destination: &str, head: &str) -> Result<(), &'static str> {
    let base = merge_base(root, destination, head);
    let base = base.as_str();
    if !crate::language_gate::check(root, base, head) {
        return Err("language gate");
    }
    if !crate::build_cache_gate::check(root, base, head) {
        return Err("build-cache gate");
    }
    if !crate::dep_gate::check_reasons(root, base, head) {
        return Err("dependency gate");
    }
    if !crate::stub_exec_gate::check(root, destination, base, head) {
        return Err("stub-exec gate");
    }
    Ok(())
}

/// The names of the lane gates that would refuse the lane and their output, judged against the lander's post-land ledger when it lives on another host.
pub(super) fn refuse_lane_gates(root: &Path, base: &str, head: &str, state_home: &Path, lander_ledger: Option<String>, registry: &dyn Registry) -> Result<Result<(), (String, String)>, String> {
    let fork = merge_base(root, base, head);
    let borrowed = lander_ledger.map(|rows| ledger_home(&rows)).transpose().map_err(|error| format!("cannot keep the lander's post-land ledger: {error}"))?;
    let code_health = crate::land::after::gates::code_health_program();
    let advisory_db = crate::advisory_gate::database(state_home);
    let lane = Lane { worktree: root, base: &fork, head, state_home: borrowed.as_ref().map_or(state_home, |home| home.path()), code_health: &code_health, registry, advisory_db: &advisory_db };
    let Some((failed, text)) = lane_gates::failed(&lane) else { return Ok(Ok(())) };
    let gates = failed.join(", ");
    let detail = format!("{gates} refused it here, as the lander would after its queue wait:\n{}", String::from_utf8_lossy(&text).trim_end());
    Ok(Err((gates, detail)))
}

pub(super) fn judge_lane_gates(environment: &super::Environment, root: &Path, branch: &str, base: &str, head: &str, lander_ledger: Option<String>) -> Result<Option<String>, String> {
    match refuse_lane_gates(root, base, head, environment.state_home, lander_ledger, &crate::dep_gate::PublicRegistries)? {
        Ok(()) => Ok(git(root, &["status", "--porcelain"]).is_ok_and(|status| status.is_empty()).then(|| merge_base(root, base, head))),
        Err((gates, detail)) => super::judged(environment, root, branch, head, &gates, Err(detail)).map(|()| None),
    }
}

fn driver_merged<'a>(root: &Path, paths: &[&'a str]) -> Result<Vec<&'a str>, String> {
    let mut arguments = vec!["check-attr", "merge", "--"];
    arguments.extend(paths);
    let attributes = git(root, &arguments)?;
    let driven: Vec<&str> = attributes.lines().filter_map(|line| line.rsplit_once(": merge: ")).filter(|(_, driver)| merge_set::DRIVERS.iter().any(|known| known.name == *driver)).map(|(path, _)| path).collect();
    Ok(paths.iter().copied().filter(|path| driven.contains(path)).collect())
}

pub(super) fn refuse_conflict(root: &Path, base: &str, destination: &str) -> Result<(), String> {
    let merged = output(root, &["merge-tree", "--write-tree", "--name-only", "--no-messages", base, "HEAD"])?;
    if merged.status.code() != Some(1) {
        return Ok(());
    }
    let listing = String::from_utf8_lossy(&merged.stdout);
    let conflicted: Vec<&str> = listing.lines().skip(1).take_while(|line| !line.is_empty()).collect();
    let merged_by_driver = driver_merged(root, &conflicted)?;
    let left: Vec<&str> = conflicted.into_iter().filter(|path| !merged_by_driver.contains(path)).collect();
    if left.is_empty() {
        return Ok(());
    }
    let short = &base[..base.len().min(9)];
    Err(format!("it conflicts with origin/{destination} at {short} on {}, which the lander would send back as a conflict after its queue wait; rebase onto origin/{destination}, resolve those files and run safe-push again", left.join(", ")))
}

pub(super) fn refuse_unfetchable(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let changed = git(root, &["diff", "--name-only", &format!("{base}..{head}"), "--", "Cargo.lock"])?;
    if changed.is_empty() {
        return Ok(());
    }
    crate::land::slots::deps::fetch_crates(root).map_err(|error| format!("this lane changes Cargo.lock and its crates cannot be fetched: {error}"))
}

pub(super) fn refuse_stale_revert(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let Ok(candidate) = stale_revert::onto(root, base, head) else { return Ok(()) };
    match stale_revert::reverted(root, base, &candidate, head) {
        Ok(found) if !found.is_empty() => Err(format!("the revert guard refused it here, as the lander would after its queue wait:\n{}", stale_revert::refusal(&found))),
        _ => Ok(()),
    }
}

fn ledger_home(rows: &str) -> std::io::Result<tempfile::TempDir> {
    let home = tempfile::tempdir()?;
    std::fs::create_dir_all(home.path().join("safe-push"))?;
    std::fs::write(home.path().join("safe-push/post-land.tsv"), rows)?;
    Ok(home)
}

pub(super) fn refuse_nothing_to_land(root: &Path, base: &str, destination: &str) -> Result<(), String> {
    if !output(root, &["merge-base", "--is-ancestor", "HEAD", base])?.status.success() {
        return Ok(());
    }
    let status = output(root, &["status", "--porcelain", "-z", "--untracked-files=all"])?;
    let listing = String::from_utf8_lossy(&status.stdout);
    let paths: Vec<&str> = listing.split('\0').filter_map(|entry| entry.get(3..)).collect();
    let uncommitted = if paths.is_empty() { String::new() } else { format!(", and these paths are uncommitted: {}", paths.join(", ")) };
    Err(format!("nothing to land: HEAD is already on origin/{destination}, so the lane adds no commit{uncommitted}; commit the lane's change and run safe-push again"))
}
