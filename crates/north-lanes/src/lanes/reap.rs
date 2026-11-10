use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::gc::{BuildTarget, drop_build_target, gib};
use super::live_process;
use super::repo::{find_container, git_checked, git_ok, git_out, is_lander_tree};
use super::verdict::{Facts, Verdict, retirement_verdict};
use north_base::platform::land_queue;

const MINIMUM_AGE_SECONDS: i64 = 7200;
const DETACHED: &str = "(detached)";
const USAGE: &str = "Usage: north lane reap [--all] [--dry-run]

Sweep spent lanes out of a container's worktrees/ directory.

A container holds exactly three slots, and the directory IS the lifecycle
policy:
  <project>/main/               the clean checkout — never swept
  <project>/worktrees/<slug>/   ephemeral lanes — the only thing this sweeps
  <project>/pins/<full-object-id>/ externally consumed — never swept, never
                                   enumerated here

A lane is reaped when it is completely clean (including untracked and
non-disposable ignored files), is at least two hours old, has no live process
working inside it, is not locked or a rescue branch, AND either:
  merged  — its HEAD is an ancestor of main; or
  equiv   — every commit it is ahead by is patch-equivalent to something
            already on main, or the lane's change adds nothing main does not
            hold (the rebase- and squash-landed lane).

Every reap is held back by ignored files that are not disposable caches
(.direnv/, result symlinks) — docs/private/ notes live there. A lane it keeps
whose tip is on main, or whose change main already holds, loses its
git-ignored target/ at once unless a process holds it open, so a landed lane
leaves no build output behind.
Both reap modes are held back by: a rescue-* branch, a locked worktree, a
live process whose CWD is inside the tree, or an age under 2h. Equivalence
reaping is also held back by any merge commit the lane is ahead by.

  (no flags)  operate on the container of the current directory
  --all       operate on every ~/code/*/main container
  --dry-run   report what would be reaped or kept; change nothing

Only <project>/worktrees/<slug> is swept. main/, pins/*, and any other shape
are skipped by construction — a pin is externally consumed, so it is never a
reap candidate no matter how clean, merged, detached, or old it is.";

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct Counts {
    pub(super) reaped: usize,
    pub(super) reaped_equivalent: usize,
    pub(super) kept_dirty: usize,
    pub(super) kept_unmerged: usize,
    pub(super) kept_protected: usize,
}

#[derive(Debug, Default)]
pub(super) struct Report {
    pub(super) lines: Vec<String>,
    pub(super) counts: Counts,
}

pub(super) struct Sweep {
    pub(super) dry_run: bool,
    pub(super) minimum_age: i64,
    pub(super) only: Option<PathBuf>,
    pub(super) now: SystemTime,
    pub(super) proc_root: PathBuf,
    pub(super) queues: PathBuf,
}

struct Lane<'a> {
    main_dir: &'a Path,
    main_ref: String,
    tree: PathBuf,
    locked: &'a [PathBuf],
}

enum Outcome {
    Retire { branch: String },
    RetireEquivalent { branch: String, head: String },
    KeepDirty,
    KeepIgnored { first: String, total: usize },
    KeepProtected { reason: String, branch: String },
    KeepUnmerged { branch: String, head: String },
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut all, mut dry_run) = (false, false);
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return 0;
            }
            "--all" => all = true,
            "--dry-run" => dry_run = true,
            other => {
                eprintln!("lane reap: unknown argument '{other}'\n{USAGE}");
                return 2;
            }
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let sweep = Sweep { dry_run, minimum_age: MINIMUM_AGE_SECONDS, only: None, now: SystemTime::now(), proc_root: PathBuf::from("/proc"), queues: land_queue::queues(&home.join(".local/state")) };
    let containers = if all { Ok(all_containers(&home)) } else { current_container() };
    let (report, failure) = match containers {
        Ok(containers) => sweep.reap_containers(&containers),
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    for line in &report.lines {
        println!("{line}");
    }
    if let Some(message) = failure {
        eprintln!("{message}");
        return 1;
    }
    println!("{}", summary(&report.counts, dry_run));
    0
}

pub(super) fn all_containers(home: &Path) -> Vec<PathBuf> {
    let mut mains = fs::read_dir(home.join("code"))
        .map(|entries| entries.flatten().map(|entry| entry.path().join("main")).collect::<Vec<_>>())
        .unwrap_or_default();
    mains.sort();
    mains.into_iter().filter(|main| main.join(".git").exists()).filter_map(|main| main.parent().map(Path::to_path_buf)).collect()
}

fn current_container() -> Result<Vec<PathBuf>, String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    find_container(&cwd)
        .map(|container| vec![container])
        .ok_or_else(|| format!("lane reap: no container found above {} (no <dir>/main/.git)", cwd.display()))
}

fn summary(counts: &Counts, dry_run: bool) -> String {
    let (prefix, reaped, equivalent) = if dry_run { ("dry-run: would reap", "", "would reap equiv") } else { ("reaped", "", "reaped equiv") };
    format!(
        "{prefix}{reaped} {}, {equivalent} {}, kept dirty {}, kept unmerged {}, kept protected {}",
        counts.reaped, counts.reaped_equivalent, counts.kept_dirty, counts.kept_unmerged, counts.kept_protected
    )
}

impl Sweep {
    pub(super) fn reap_containers(&self, containers: &[PathBuf]) -> (Report, Option<String>) {
        let mut report = Report::default();
        for container in containers {
            if let Err(message) = self.reap_container(container, &mut report) {
                return (report, Some(message));
            }
        }
        (report, None)
    }

    fn reap_container(&self, container: &Path, report: &mut Report) -> Result<(), String> {
        let Ok(container) = container.canonicalize() else {
            return Ok(());
        };
        let main_dir = container.join("main");
        if !main_dir.join(".git").exists() {
            return Ok(());
        }
        if !self.dry_run {
            git_checked(&main_dir, &["worktree", "prune"])?;
        }
        let Some(main_branch) = git_out(&main_dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]) else {
            return Ok(());
        };
        let listing = git_out(&main_dir, &["worktree", "list", "--porcelain"]).unwrap_or_default();
        let (trees, locked) = parse_listing(&listing);
        let sweep_dir = container.join("worktrees");
        let main_ref = git_out(&main_dir, &["rev-parse", "--symbolic-full-name", &format!("{main_branch}@{{upstream}}")]).unwrap_or_else(|| format!("refs/heads/{main_branch}"));
        for tree in trees.into_iter().filter(|tree| tree.parent() == Some(sweep_dir.as_path()) && !is_lander_tree(tree) && self.only.as_ref().is_none_or(|only| only == tree)) {
            let lane = Lane { main_dir: &main_dir, main_ref: main_ref.clone(), tree, locked: &locked };
            if let Some(outcome) = self.judge(&lane) {
                self.act(&lane, outcome, report)?;
            }
        }
        Ok(())
    }

    fn judge(&self, lane: &Lane) -> Option<Outcome> {
        let tree = lane.tree.as_path();
        let dirty = git_out(tree, &["--no-optional-locks", "status", "--porcelain"])?;
        if !dirty.is_empty() {
            return Some(Outcome::KeepDirty);
        }
        let ignored = irreplaceable_ignored(tree);
        if let Some(first) = ignored.first() {
            return Some(Outcome::KeepIgnored { first: first.clone(), total: ignored.len() });
        }
        let head = git_out(tree, &["rev-parse", "HEAD"])?;
        let branch_ref = git_out(tree, &["symbolic-ref", "--quiet", "HEAD"]).unwrap_or_default();
        let branch = branch_ref.strip_prefix("refs/heads/").filter(|name| !name.is_empty()).unwrap_or(DETACHED).to_string();
        if let Some(reason) = self.safety_gate(lane, &branch) {
            return Some(Outcome::KeepProtected { reason, branch });
        }
        let age = lane_age(tree, self.now);
        let facts = self.facts(lane, &head, &branch, &branch_ref, age);
        Some(match retirement_verdict(&facts) {
            Verdict::Retire => Outcome::Retire { branch },
            Verdict::RetireEquivalent => Outcome::RetireEquivalent { branch, head },
            Verdict::Keep | Verdict::ArchiveThenDelete if age < self.minimum_age => {
                Outcome::KeepProtected { reason: "fresh".into(), branch }
            }
            Verdict::Keep | Verdict::ArchiveThenDelete => Outcome::KeepUnmerged { branch, head },
        })
    }

    fn safety_gate(&self, lane: &Lane, branch: &str) -> Option<String> {
        let reason = if branch.starts_with("rescue-") {
            "rescue"
        } else if lane.locked.contains(&lane.tree) {
            "locked"
        } else if live_process::has_cwd_inside(&self.proc_root, &lane.tree) {
            "live cwd"
        } else if !land_queue::waiting_on(&self.queues, &land_queue::local_host(), &lane.tree).is_empty() {
            "land ticket waiting"
        } else {
            return None;
        };
        Some(reason.to_string())
    }

    fn facts(&self, lane: &Lane, head: &str, branch: &str, branch_ref: &str, age: i64) -> Facts {
        let mut facts = Facts {
            age,
            minimum_age: self.minimum_age,
            merged: false,
            equivalent: false,
            evil_merge: false,
            archive_age: None,
            protections: 0,
        };
        if age < self.minimum_age {
            return facts;
        }
        if git_ok(lane.main_dir, &["merge-base", "--is-ancestor", head, &lane.main_ref]) {
            facts.merged = true;
        } else if branch != DETACHED {
            let range = format!("{}..{branch_ref}", lane.main_ref);
            facts.evil_merge = git_out(lane.main_dir, &["rev-list", "--merges", &range]).is_none_or(|merges| !merges.is_empty());
            facts.equivalent = equivalent_to_main(lane.main_dir, &lane.main_ref, &format!("refs/heads/{branch}"));
        }
        facts
    }

    fn act(&self, lane: &Lane, outcome: Outcome, report: &mut Report) -> Result<(), String> {
        let tree = lane.tree.display();
        let counts = &mut report.counts;
        let (line, kept) = match outcome {
            Outcome::Retire { branch } => {
                counts.reaped += 1;
                if self.dry_run {
                    (format!("would reap: {tree} ({branch})"), false)
                } else {
                    self.remove(lane, &branch)?;
                    (format!("reaped: {tree} ({branch})"), false)
                }
            }
            Outcome::RetireEquivalent { branch, head } => {
                counts.reaped_equivalent += 1;
                if self.dry_run {
                    (format!("would reap (equiv): {tree} ({branch} was {head})"), false)
                } else {
                    self.remove(lane, &branch)?;
                    (format!("reaped (equiv): {tree} ({branch} was {head})"), false)
                }
            }
            Outcome::KeepDirty => {
                counts.kept_dirty += 1;
                (format!("kept: dirty ({tree})"), true)
            }
            Outcome::KeepIgnored { first, total } => {
                counts.kept_protected += 1;
                (format!("kept: ignored files ({first}, {total} total) ({tree})"), true)
            }
            Outcome::KeepProtected { reason, branch } => {
                counts.kept_protected += 1;
                (format!("kept: {reason} ({branch}) ({tree})"), true)
            }
            Outcome::KeepUnmerged { branch, head } => {
                counts.kept_unmerged += 1;
                let range = format!("{}..{head}", lane.main_ref);
                let ahead = git_out(lane.main_dir, &["rev-list", "--count", &range]).unwrap_or_else(|| "?".into());
                (format!("kept: unmerged ({branch}, {ahead} commits ahead) ({tree})"), true)
            }
        };
        report.lines.push(line);
        if kept {
            self.drop_landed_target(lane, report);
        }
        Ok(())
    }

    fn drop_landed_target(&self, lane: &Lane, report: &mut Report) {
        if !self.landed(lane) {
            return;
        }
        let target = lane.tree.join("target");
        match drop_build_target(&lane.tree, &[], &self.proc_root, self.dry_run) {
            BuildTarget::Absent | BuildTarget::InUse => {}
            BuildTarget::Dropped(bytes) => report.lines.push(format!(
                "{} build target {} of the landed lane ({})",
                if self.dry_run { "would drop" } else { "dropped" },
                target.display(),
                gib(bytes)
            )),
            BuildTarget::Failed(error) => report.lines.push(format!("kept build target {} of the landed lane: {error}", target.display())),
        }
    }

    fn landed(&self, lane: &Lane) -> bool {
        let Some(head) = git_out(&lane.tree, &["rev-parse", "HEAD"]) else { return false };
        if git_ok(lane.main_dir, &["merge-base", "--is-ancestor", &head, &lane.main_ref]) {
            return true;
        }
        let branch_ref = git_out(&lane.tree, &["symbolic-ref", "--quiet", "HEAD"]).unwrap_or_default();
        !branch_ref.is_empty() && equivalent_to_main(lane.main_dir, &lane.main_ref, &branch_ref)
    }

    fn remove(&self, lane: &Lane, branch: &str) -> Result<(), String> {
        git_checked(lane.main_dir, &["worktree", "remove", &lane.tree.to_string_lossy()])?;
        if branch != DETACHED {
            git_checked(lane.main_dir, &["branch", "-D", branch])?;
        }
        Ok(())
    }
}

fn parse_listing(listing: &str) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut trees, mut locked) = (Vec::new(), Vec::new());
    for line in listing.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            trees.push(PathBuf::from(path));
        } else if line.starts_with("locked") {
            locked.extend(trees.last().cloned());
        }
    }
    (trees, locked)
}

pub(super) fn irreplaceable_ignored(tree: &Path) -> Vec<String> {
    git_out(tree, &["ls-files", "--others", "--ignored", "--exclude-standard"])
        .unwrap_or_default()
        .lines()
        .filter(|path| !path.is_empty() && !disposable(path))
        .map(str::to_string)
        .collect()
}

fn disposable(path: &str) -> bool {
    let cache = |name: &str| path.starts_with(&format!("{name}/")) || path.contains(&format!("/{name}/"));
    cache(".direnv") || cache(".firn-cache") || cache("target") || path == "target" || ((path == "result" || path.starts_with("result-")) && !path.contains('/'))
}

fn lane_age(tree: &Path, now: SystemTime) -> i64 {
    let born = fs::metadata(tree.join(".git")).and_then(|meta| meta.modified()).ok();
    let seconds = |time: SystemTime| time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX));
    born.map_or(MINIMUM_AGE_SECONDS, |born| seconds(now) - seconds(born))
}

fn equivalent_to_main(main_dir: &Path, main_ref: &str, branch_ref: &str) -> bool {
    let every_commit = git_out(main_dir, &["cherry", main_ref, branch_ref]).is_some_and(|cherry| !cherry.lines().any(|line| line.starts_with('+')));
    every_commit || adds_nothing(main_dir, main_ref, branch_ref)
}

fn adds_nothing(main_dir: &Path, main_ref: &str, branch_ref: &str) -> bool {
    let Some(main_tree) = git_out(main_dir, &["rev-parse", &format!("{main_ref}^{{tree}}")]) else {
        return false;
    };
    git_out(main_dir, &["merge-tree", "--write-tree", main_ref, branch_ref]).is_some_and(|merged| merged.lines().next() == Some(main_tree.as_str()))
}
