use std::fs;
use std::path::{Path, PathBuf};

use super::exec::git;

pub const USAGE: &str = "Usage: north lane restore [--archive DIR] BRANCH

Restores the newest archived copy of BRANCH (north lane gc archives an unheld lane
older than a day: its commits, and its uncommitted and untracked changes, go to a
bundle and its worktree is removed) to a worktree. The worktree is the lane's old path
when that is free, else <container>/worktrees/ named by the branch. The branch is
created at the archived commit, or used when it already sits there; the archived
changes come back as unstaged changes, as they were.

  --archive DIR  the archive to read (default ~/archive/lanes)";

const SNAPSHOT_SUBJECTS: [&str; 2] = ["north lane gc: uncommitted changes of", "north lane gc: files of the unregistered"];

struct Row {
    at: String,
    place: String,
    head: String,
    file: PathBuf,
    project: String,
}

pub fn run(arguments: &[String]) -> u8 {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let Ok((archive, branch)) = parse(arguments, &home) else {
        return if arguments.iter().any(|word| word == "-h" || word == "--help") {
            println!("{USAGE}");
            0
        } else {
            eprintln!("{USAGE}");
            2
        };
    };
    match restore(&home, &archive, &branch) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(error) => {
            eprintln!("lane restore: {error}");
            1
        }
    }
}

fn parse(arguments: &[String], home: &Path) -> Result<(PathBuf, String), ()> {
    let mut archive = home.join("archive/lanes");
    let mut branch = None;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--archive" => archive = PathBuf::from(words.next().ok_or(())?),
            word if branch.is_none() && !word.starts_with('-') => branch = Some(word.to_owned()),
            _ => return Err(()),
        }
    }
    Ok((archive, branch.ok_or(())?))
}

pub fn restore(home: &Path, archive: &Path, branch: &str) -> Result<String, String> {
    let row = newest(archive, branch)?;
    let main = home.join("code").join(&row.project).join("main");
    let file = row.file.to_string_lossy().into_owned();
    fetch_head(&main, &file, &row.head)?;
    let tip = lane_tip(&main, &row.head)?;
    let existing = git(&main, &["rev-parse", "-q", "--verify", &format!("refs/heads/{branch}")]);
    if existing.ok && existing.out != tip {
        return Err(format!("branch {branch} is at {} but the archive holds {tip}; move the branch first", existing.out));
    }
    let path = worktree_path(home, &row, branch);
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    add_worktree(&main, &path, &row.head, &tip, branch, existing.ok)?;
    Ok(format!("restored {branch} to {} from {file} (archived {}); its uncommitted changes are back as unstaged changes", path.display(), row.at))
}

fn fetch_head(main: &Path, file: &str, head: &str) -> Result<(), String> {
    let verified = git(main, &["bundle", "verify", "-q", file]);
    if !verified.ok {
        return Err(format!("{file} does not verify: {}", verified.last_err_line()));
    }
    let heads = git(main, &["bundle", "list-heads", file]);
    let Some(source) = heads.out.lines().find_map(|line| line.split_once(' ').filter(|(sha, _)| *sha == head).map(|(_, name)| name.to_owned())) else {
        return Err(format!("{file} does not hold {head}"));
    };
    let fetched = git(main, &["fetch", "-q", "--no-tags", file, &source]);
    if !fetched.ok {
        return Err(format!("fetch from {file} failed: {}", fetched.last_err_line()));
    }
    Ok(())
}

fn lane_tip(main: &Path, head: &str) -> Result<String, String> {
    let subject = git(main, &["log", "-1", "--format=%s", head]).out;
    if !SNAPSHOT_SUBJECTS.iter().any(|prefix| subject.starts_with(prefix)) {
        return Ok(head.to_owned());
    }
    let parent = git(main, &["rev-parse", "--verify", "-q", &format!("{head}^")]);
    if !parent.ok {
        return Err(format!("the snapshot {head} has no lane commit under it"));
    }
    Ok(parent.out)
}

fn worktree_path(home: &Path, row: &Row, branch: &str) -> PathBuf {
    let place = PathBuf::from(&row.place);
    if row.place.starts_with('/') && !place.exists() {
        return place;
    }
    home.join("code").join(&row.project).join("worktrees").join(slug(branch))
}

fn add_worktree(main: &Path, path: &Path, head: &str, tip: &str, branch: &str, existing: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let worktree = path.to_string_lossy().into_owned();
    let added = git(main, &["worktree", "add", "--detach", &worktree, head]);
    if !added.ok {
        return Err(format!("worktree add failed: {}", added.last_err_line()));
    }
    let unstaged = git(path, &["reset", "-q", "--mixed", tip]);
    if !unstaged.ok {
        return Err(format!("{} holds the archived changes, but unstaging them failed: {}", path.display(), unstaged.last_err_line()));
    }
    let switched = if existing { git(path, &["switch", "-q", branch]) } else { git(path, &["switch", "-q", "-c", branch]) };
    if !switched.ok {
        return Err(format!("{} holds the archived changes, but switching to {branch} failed: {}", path.display(), switched.last_err_line()));
    }
    Ok(())
}

fn newest(archive: &Path, branch: &str) -> Result<Row, String> {
    let projects = fs::read_dir(archive).map_err(|error| format!("{}: {error}", archive.display()))?;
    let mut found: Option<Row> = None;
    for project in projects.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(index) = fs::read_to_string(project.path().join("index.tsv")) else { continue };
        for line in index.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let [at, place, lane, head, _, _, file] = fields.as_slice() else { continue };
            if *lane != branch || *file == "-" || found.as_ref().is_some_and(|best| best.at.as_str() >= *at) {
                continue;
            }
            found = Some(Row { at: (*at).to_owned(), place: (*place).to_owned(), head: (*head).to_owned(), file: PathBuf::from(file), project: project.file_name().to_string_lossy().into_owned() });
        }
    }
    found.ok_or_else(|| format!("no archived lane for {branch} under {}", archive.display()))
}

fn slug(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect()
}
