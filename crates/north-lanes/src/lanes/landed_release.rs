//! Releases a landed lane's build directory as soon as nothing holds the lane: the lanes come from the landed tickets this host's
//! records keep, never from a listing, and each is judged again at the moment of removal.
use std::fs;
use std::path::{Path, PathBuf};

use super::gc::{BuildTarget, drop_build_target};
use super::landed_tickets::LandedTickets;
use super::live_process::{any_inside, open_paths};
use super::reap::parse_listing;
use super::repo::{git_out, is_lander_tree};

pub struct ReleasedTarget {
    pub path: PathBuf,
    pub bytes: u64,
}

/// Drops the git-ignored `target/` of every lane under `code/PROJECT/worktrees/` that a landed ticket from this host names, while the
/// lane still sits at the ticket's head, the commit the lander made is on main, the worktree is not locked and no process has its cwd,
/// its root or a file open anywhere in the lane.
pub fn release_landed_targets(code: &Path, state_home: &Path, proc_root: &Path, dry_run: bool) -> Vec<ReleasedTarget> {
    let mut released = Vec::new();
    let mut held: Option<Vec<PathBuf>> = None;
    for main in checkouts(code) {
        let tickets = LandedTickets::read(state_home, &main);
        let Some(worktrees) = main.parent().map(|container| container.join("worktrees")) else { continue };
        let candidates: Vec<PathBuf> = tickets.roots().filter_map(|root| fs::canonicalize(root).ok()).filter(|root| root.parent() == Some(worktrees.as_path()) && !is_lander_tree(root) && root.join("target").is_dir()).collect();
        if candidates.is_empty() {
            continue;
        }
        let Some(branch) = git_out(&main, &["symbolic-ref", "--quiet", "--short", "HEAD"]) else { continue };
        let base = git_out(&main, &["rev-parse", "--symbolic-full-name", &format!("{branch}@{{upstream}}")]).unwrap_or_else(|| format!("refs/heads/{branch}"));
        let (_, locked) = parse_listing(&git_out(&main, &["worktree", "list", "--porcelain"]).unwrap_or_default());
        for lane in candidates {
            let Some(head) = git_out(&lane, &["rev-parse", "HEAD"]) else { continue };
            if locked.contains(&lane) || tickets.landed(&main, &base, &lane, &head).is_none() {
                continue;
            }
            if any_inside(held.get_or_insert_with(|| holders(proc_root)), &lane) {
                continue;
            }
            if let BuildTarget::Dropped(bytes) = drop_build_target(&lane, &[], proc_root, dry_run) {
                released.push(ReleasedTarget { path: lane.join("target"), bytes });
            }
        }
    }
    released.sort_by(|left, right| left.path.cmp(&right.path));
    released
}

fn holders(proc_root: &Path) -> Vec<PathBuf> {
    let mut paths = open_paths(proc_root);
    let processes = fs::read_dir(proc_root).into_iter().flatten().flatten().filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()));
    paths.extend(processes.filter_map(|process| fs::read_link(process.path().join("root")).ok()).filter(|root| root != Path::new("/")));
    paths
}

fn checkouts(code: &Path) -> Vec<PathBuf> {
    let mut mains: Vec<PathBuf> = fs::read_dir(code).into_iter().flatten().flatten().map(|entry| entry.path().join("main")).filter(|main| main.join(".git").exists()).filter_map(|main| fs::canonicalize(main).ok()).collect();
    mains.sort();
    mains
}
