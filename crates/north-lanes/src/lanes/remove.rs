use std::path::{Path, PathBuf};
use std::time::SystemTime;

use north_base::foundation::clock::Timestamp;
use north_base::platform::land_queue;

use super::nested_shell;
use super::orphan;
use super::reap::Sweep;
use super::reject::reject;
use super::repo::{find_container, git_out, is_lander_tree};

const USAGE: &str = "usage: north lane remove [--rejected VERDICT] LANE...

Removes each LANE, a directory directly under a project's worktrees/ (named by its absolute path),
only when nothing in it is at risk. A registered worktree must be clean, hold no ignored file
beyond build output and caches, have every commit on main or a change main already holds,
and be neither locked, nor waiting in the land queue, nor the cwd of a live process: it is removed
with its branch, as `north lane reap` does but with no minimum age. A lane it keeps whose tip is
on main, or whose change main already holds, loses its git-ignored target/ at once unless a
process holds it open, so a landed lane leaves no build output behind. A directory git no longer
registers (left behind when `git worktree remove` could not delete it) must have no .git or a .git file
whose git directory is gone, no live process and no waiting ticket, and every file it holds that
git does not ignore must already be stored in the repository's object database. Anything else is
kept and the reason named. A lane nested inside a main checkout (main/PROJECT/worktrees/NAME, left when a relative lane path met git -C) is removed only when it is an empty shell holding just its .git file. It is the removal a sandboxed worker may make of its own finished lane.

With --rejected VERDICT, a lane a reviewer rejected goes although it holds unlanded work and its
issue is open: it is bundled to ~/archive/lanes/PROJECT/ with VERDICT in index.tsv, as `north lane gc`
archives a lane whose issue closed (uncommitted changes ride as a snapshot commit), and only once
the bundle verifies are its worktree and branch removed. A directory git no longer registers is
bundled from the branch named like it. A lane that is locked, waited on by a land ticket or the cwd
of a live process is kept. This is the command a reviewer runs when it rejects a lane.";

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let (verdict, lanes) = match arguments {
        [flag, verdict, lanes @ ..] if flag == "--rejected" => (Some(verdict.as_str()), lanes),
        lanes => (None, lanes),
    };
    if lanes.is_empty() || lanes.iter().any(|argument| argument.starts_with('-')) {
        eprintln!("{USAGE}");
        return 2;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let queues = land_queue::queues(&home.join(".local/state"));
    let mut code = 0;
    for lane in lanes.iter().map(PathBuf::from) {
        match settle(&lane, verdict, &home, &queues) {
            Ok(message) => println!("{message}"),
            Err(reason) => {
                eprintln!("kept {}: {reason}", lane.display());
                code = 1;
            }
        }
    }
    code
}

fn settle(lane: &Path, verdict: Option<&str>, home: &Path, queues: &Path) -> Result<String, String> {
    match verdict {
        Some(verdict) => reject(lane, verdict, queues, &home.join("archive/lanes"), Timestamp::now().unix_millis()),
        None => remove(lane, queues),
    }
}

pub(super) fn remove(lane: &Path, queues: &Path) -> Result<String, String> {
    if !lane.is_absolute() {
        return Err("name it by its absolute path".to_owned());
    }
    let meta = std::fs::symlink_metadata(lane).map_err(|error| error.to_string())?;
    if !meta.file_type().is_dir() {
        return Err("not a directory".to_owned());
    }
    let container = lane.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "worktrees")).and_then(Path::parent).ok_or("not a directory directly under a project's worktrees/")?;
    if let Some(nested) = nested_shell::real_main(container) {
        return nested_shell::remove(nested, lane);
    }
    let main = container.join("main");
    if !main.join(".git").exists() || find_container(lane) != Some(container.to_path_buf()) || is_lander_tree(lane) {
        return Err("not a lane of a project container (no main/.git beside worktrees/)".to_owned());
    }
    let listing = git_out(&main, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    if listing.lines().filter_map(|line| line.strip_prefix("worktree ")).any(|path| Path::new(path) == lane) {
        let sweep = Sweep { dry_run: false, minimum_age: 0, only: Some(lane.to_path_buf()), now: SystemTime::now(), proc_root: PathBuf::from("/proc"), queues: queues.to_path_buf() };
        let (report, failure) = sweep.reap_containers(&[container.to_path_buf()]);
        return match failure {
            Some(message) => Err(message),
            None if report.counts.reaped + report.counts.reaped_equivalent > 0 && !lane.exists() => Ok(report.lines.join("\n")),
            None => Err(report.lines.join("; ")),
        };
    }
    orphan::remove(&main, lane, queues)
}

