use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use north_base::platform::land_queue;

use super::live_process;
use super::nested_shell;
use super::reap::Sweep;
use super::repo::{find_container, git_out, is_lander_tree};

const USAGE: &str = "usage: north lane remove LANE...

Removes each LANE, a directory directly under a project's worktrees/ (named by its absolute path),
only when nothing in it is at risk. A registered worktree must be clean, hold no ignored file
beyond build output and caches, have every commit on main or patch-equivalent to a commit there,
and be neither locked, nor waiting in the land queue, nor the cwd of a live process: it is removed
with its branch, as `north lane reap` does but with no minimum age. A directory git no longer
registers (left behind when `git worktree remove` could not delete it) must have no .git or a .git file
whose git directory is gone, no live process and no waiting ticket, and every file it holds that
git does not ignore must already be stored in the repository's object database. Anything else is
kept and the reason named. A lane nested inside a main checkout (main/PROJECT/worktrees/NAME, left when a relative lane path met git -C) is removed only when it is an empty shell holding just its .git file. It is the removal a sandboxed worker may make of its own finished lane.";

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    if arguments.is_empty() || arguments.iter().any(|argument| argument.starts_with('-')) {
        eprintln!("{USAGE}");
        return 2;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let queues = land_queue::queues(&home.join(".local/state"));
    let mut code = 0;
    for lane in arguments.iter().map(PathBuf::from) {
        match remove(&lane, &queues) {
            Ok(message) => println!("{message}"),
            Err(reason) => {
                eprintln!("kept {}: {reason}", lane.display());
                code = 1;
            }
        }
    }
    code
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
    remove_orphan(&main, lane, queues)
}

fn remove_orphan(main: &Path, lane: &Path, queues: &Path) -> Result<String, String> {
    if std::fs::read_dir(lane).map_err(|error| error.to_string())?.next().is_none() {
        std::fs::remove_dir(lane).map_err(|error| format!("cannot delete it: {error}"))?;
        return Ok(format!("removed: {} (empty)", lane.display()));
    }
    match std::fs::read_to_string(lane.join(".git")) {
        Ok(gitfile) => {
            let gitdir = gitfile.trim().strip_prefix("gitdir: ").ok_or("its .git is not a gitfile")?;
            if Path::new(gitdir).exists() {
                return Err(format!("its git directory {gitdir} still exists, so another repository registers it"));
            }
        }
        Err(_) if lane.join(".git").exists() => return Err("its .git is a directory, so it is a clone, not a worktree".to_owned()),
        Err(_) => {}
    }
    if live_process::has_cwd_inside(Path::new("/proc"), lane) {
        return Err("a live process has its cwd inside".to_owned());
    }
    if !land_queue::waiting_on(queues, &land_queue::local_host(), lane).is_empty() {
        return Err("a land ticket waits on it".to_owned());
    }
    if let Some(unstored) = unstored_file(&main.join(".git"), lane)? {
        return Err(format!("{unstored} holds content the repository does not store"));
    }
    std::fs::remove_dir_all(lane).map_err(|error| format!("cannot delete it: {error}"))?;
    Ok(format!("removed: {} (git no longer registered it and every file it held is stored in the repository)", lane.display()))
}

fn unstored_file(git_dir: &Path, lane: &Path) -> Result<Option<String>, String> {
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let git = |arguments: &[&str], input: &[u8]| -> Result<Vec<u8>, String> {
        let mut child = Command::new("git")
            .arg("--git-dir")
            .arg(git_dir)
            .arg("--work-tree")
            .arg(lane)
            .args(arguments)
            .current_dir(lane)
            .env("GIT_INDEX_FILE", scratch.path().join("index"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let input = input.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output().map_err(|error| error.to_string())?;
        let _ = writer.join();
        output.status.success().then_some(output.stdout).ok_or_else(|| format!("git {} failed", arguments.join(" ")))
    };
    let listed = git(&["ls-files", "-o", "--exclude-standard", "-z"], b"")?;
    let files: Vec<String> = listed.split(|byte| *byte == 0).filter(|name| !name.is_empty()).map(|name| String::from_utf8_lossy(name).into_owned()).collect();
    let regular: Vec<&String> = files.iter().filter(|name| std::fs::symlink_metadata(lane.join(name)).is_ok_and(|meta| meta.file_type().is_file())).collect();
    if regular.iter().any(|name| name.contains('\n')) {
        return Err("a file name holds a newline".to_owned());
    }
    let paths: String = regular.iter().map(|name| format!("{}\n", lane.join(name).display())).collect();
    let hashed = String::from_utf8_lossy(&git(&["hash-object", "--stdin-paths"], paths.as_bytes())?).into_owned();
    let object_ids: Vec<&str> = hashed.lines().collect();
    let checked = String::from_utf8_lossy(&git(&["cat-file", "--batch-check"], hashed.as_bytes())?).into_owned();
    Ok(checked.lines().position(|line| line.ends_with(" missing")).and_then(|at| regular.get(at)).map(|name| (*name).clone()).or_else(|| (object_ids.len() != regular.len()).then(|| "a file".to_owned())))
}
