use std::path::Path;
use std::process::{Command, Stdio};

use crate::land::candidate::git;
use crate::land::lane_gates::{self, Judging, Lane};
use crate::land::scans::git::merge_base;
use crate::landing::stale_revert;
use crate::watch::facts::slug_of;

pub(super) fn refuse_archived(origin: &str) -> Result<(), String> {
    let Some(slug) = origin.contains("github.com").then(|| slug_of(origin)).flatten() else { return Ok(()) };
    let answer = Command::new("timeout").args(["10", "gh", "api", &format!("repos/{slug}"), "--jq", ".archived"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    match answer {
        Ok(output) if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" => Err(format!("GitHub reports {slug} as archived, so origin refuses every push; unarchive it or land elsewhere")),
        _ => Ok(()),
    }
}

pub(in crate::land) fn refuse_static_gates(root: &Path, destination: &str, head: &str) -> Result<(), String> {
    let base = merge_base(root, destination, head);
    let base = base.as_str();
    if !crate::language_gate::check(root, base, head) {
        return Err("language gate refused the landing".to_owned());
    }
    if !crate::build_cache_gate::check(root, base, head) {
        return Err("build-cache gate refused the landing".to_owned());
    }
    if !crate::dep_gate::check_reasons(root, base, head) {
        return Err("dependency gate refused the landing".to_owned());
    }
    if !crate::stub_exec_gate::check(root, destination, base, head) {
        return Err("stub-exec gate refused the landing".to_owned());
    }
    Ok(())
}

pub(super) fn refuse_lane_gates(root: &Path, base: &str, head: &str, state_home: &Path, lander_ledger: Option<String>) -> Result<(), String> {
    let fork = merge_base(root, base, head);
    let borrowed = lander_ledger.map(|rows| ledger_home(&rows)).transpose().map_err(|error| format!("cannot keep the lander's post-land ledger: {error}"))?;
    let lane = Lane { worktree: root, base: &fork, head, state_home: borrowed.as_ref().map_or(state_home, |home| home.path()) };
    let Some((failed, text)) = lane_gates::failed(&lane, Judging::Recorded) else { return Ok(()) };
    Err(format!("{} refused it here, as the lander would after its queue wait:\n{}", failed.join(", "), String::from_utf8_lossy(&text).trim_end()))
}

pub(super) fn refuse_unfetchable(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let changed = git(root, &["diff", "--name-only", &format!("{base}..{head}"), "--", "Cargo.lock"])?;
    if changed.is_empty() {
        return Ok(());
    }
    crate::land::slots::deps::fetch_crates(root).map_err(|error| format!("this lane changes Cargo.lock and its crates cannot be fetched: {error}"))
}

pub(super) fn refuse_stale_revert(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let Ok(merged) = git(root, &["merge-tree", "--write-tree", base, head]) else { return Ok(()) };
    let Some(tree) = merged.lines().next() else { return Ok(()) };
    let Ok(candidate) = git(root, &["commit-tree", tree, "-p", base, "-p", head, "-m", "the lane merged onto main"]) else { return Ok(()) };
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
