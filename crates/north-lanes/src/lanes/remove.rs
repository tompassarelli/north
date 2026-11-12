use std::path::{Path, PathBuf};
use std::time::SystemTime;

use north_base::foundation::clock::Timestamp;
use north_base::platform::land_queue;

use super::nested_shell;
use super::orphan;
use super::reap::Sweep;
use super::reject::{reject, reject_branch};
use super::repo::{find_container, git_out, is_lander_tree};

const USAGE: &str = "usage: north lane remove [--rejected VERDICT] LANE...
       north lane remove --rejected VERDICT --branch NAME...

Removes each LANE, a directory directly under a project's worktrees/ (named by its absolute path),
only when nothing in it is at risk. A registered worktree must be clean, hold no ignored file
beyond build output and caches, have every commit on main, a change main already holds or a landed ticket from this host naming
the head it still has, and be neither locked, nor waiting in the land queue, nor the cwd of a live process: it is removed
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
of a live process is kept. This is the command a reviewer runs when it rejects a lane.

With --branch NAME (repeatable, with --rejected VERDICT), the local branch NAME of the project containing the
current directory is bundled the same way and deleted once the bundle verifies, when no worktree has it checked out.
A slashed name such as codex/x-1 bundles to one file, its slashes written as dashes.";

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some(request) = Request::parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = home.join(".local/state");
    let mut code = 0;
    for lane in &request.lanes {
        report(lane, settle(Path::new(lane), request.verdict, &home, &state), &mut code);
    }
    for branch in &request.branches {
        report(&format!("branch {branch}"), settle_branch(branch, request.verdict.unwrap_or_default(), &home), &mut code);
    }
    code
}

struct Request<'a> {
    verdict: Option<&'a str>,
    lanes: Vec<&'a str>,
    branches: Vec<&'a str>,
}

impl<'a> Request<'a> {
    fn parse(arguments: &'a [String]) -> Option<Self> {
        let mut request = Request { verdict: None, lanes: Vec::new(), branches: Vec::new() };
        let mut words = arguments.iter().map(String::as_str);
        while let Some(word) = words.next() {
            match word {
                "--rejected" => request.verdict = Some(words.next()?),
                "--branch" => request.branches.push(words.next()?),
                flag if flag.starts_with('-') => return None,
                lane => request.lanes.push(lane),
            }
        }
        let valid = if request.branches.is_empty() { !request.lanes.is_empty() } else { request.lanes.is_empty() };
        valid.then_some(request)
    }
}

fn report(subject: &str, outcome: Result<String, String>, code: &mut u8) {
    match outcome {
        Ok(message) => println!("{message}"),
        Err(reason) => {
            eprintln!("kept {subject}: {reason}");
            *code = 1;
        }
    }
}

fn settle(lane: &Path, verdict: Option<&str>, home: &Path, state: &Path) -> Result<String, String> {
    match verdict {
        Some(verdict) => reject(lane, verdict, &land_queue::queues(state), &home.join("archive/lanes"), Timestamp::now().unix_millis()),
        None => remove(lane, state),
    }
}

fn settle_branch(branch: &str, verdict: &str, home: &Path) -> Result<String, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let container = find_container(&current).ok_or("not inside a project checkout (no main/.git above the current directory)")?;
    reject_branch(&container, branch, verdict, &home.join("archive/lanes"), Timestamp::now().unix_millis())
}

pub(super) fn remove(lane: &Path, state: &Path) -> Result<String, String> {
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
        let sweep = Sweep { dry_run: false, minimum_age: 0, only: Some(lane.to_path_buf()), now: SystemTime::now(), proc_root: PathBuf::from("/proc"), state: state.to_path_buf() };
        let (report, failure) = sweep.reap_containers(&[container.to_path_buf()]);
        return match failure {
            Some(message) => Err(message),
            None if report.counts.reaped + report.counts.reaped_equivalent > 0 && !lane.exists() => Ok(report.lines.join("\n")),
            None => Err(report.lines.join("; ")),
        };
    }
    orphan::remove(&main, lane, &land_queue::queues(state))
}

