use std::path::{Path, PathBuf};

use super::lane_issue;
use super::repo::{git_checked, git_ok, git_out, is_lander_tree};

const USAGE: &str = "usage: north lane new --issue REPO#N [--from REF] LANE

Creates the lane LANE, a directory named by its absolute path directly under a project's
worktrees/ (~/code/<project>/worktrees/SLUG), on a new branch SLUG from REF (default
origin/main, else main), and records the issue it serves under the project repository's
.git/north/lane-issues/SLUG. The branch does not track REF, so concurrent lanes never write
the repository's shared git config. REPO#N names the issue (north#995); #N names one in the
project's own origin repository. lane gc reads that record first to decide the lane's fate
by its issue's state and lists the lane under it while it is open.";

struct Request {
    issue: String,
    from: Option<String>,
    lane: PathBuf,
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let (mut issue, mut from, mut lane) = (None, None, None);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--issue" => issue = words.next().cloned(),
            "--from" => from = words.next().cloned(),
            other if !other.starts_with('-') && lane.is_none() => lane = Some(PathBuf::from(other)),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let (Some(issue), Some(lane)) = (issue, lane) else { return Err(USAGE.to_owned()) };
    Ok(Request { issue, from, lane })
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let request = match parse(arguments) {
        Ok(request) => request,
        Err(usage) => {
            eprintln!("{usage}");
            return 2;
        }
    };
    match create(&request.lane, &request.issue, request.from.as_deref()) {
        Ok(issue) => {
            println!("{} ({issue})", request.lane.display());
            0
        }
        Err(reason) => {
            eprintln!("north lane new: {reason}");
            1
        }
    }
}

fn origin_name(main: &Path) -> Option<String> {
    let url = git_out(main, &["remote", "get-url", "origin"])?;
    let name = url.trim_end_matches('/').rsplit(['/', ':']).next()?.trim_end_matches(".git").to_owned();
    (!name.is_empty()).then_some(name)
}

fn qualified(main: &Path, issue: &str) -> Result<String, String> {
    let item = if issue.starts_with('#') {
        let repo = origin_name(main).ok_or("#N needs an origin remote to name its repository; give REPO#N")?;
        format!("{repo}{issue}")
    } else {
        issue.to_owned()
    };
    if lane_issue::valid(&item) { Ok(item) } else { Err(format!("{issue} is not an issue; give REPO#N such as north#995")) }
}

pub fn create(lane: &Path, issue: &str, from: Option<&str>) -> Result<String, String> {
    if !lane.is_absolute() {
        return Err("name the lane by its absolute path".to_owned());
    }
    let worktrees = lane.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "worktrees")).ok_or("a lane lives directly under a project's worktrees/")?;
    let main = worktrees.parent().map(|container| container.join("main")).filter(|main| main.join(".git").exists()).ok_or("no main/.git beside worktrees/")?;
    let slug = lane.file_name().and_then(|name| name.to_str()).filter(|name| !name.starts_with('.') && !is_lander_tree(Path::new(name))).ok_or("the lane needs a plain directory name")?;
    if lane.exists() {
        return Err(format!("{} exists", lane.display()));
    }
    let item = qualified(&main, issue)?;
    let base = from.map(str::to_owned).or_else(|| ["origin/main", "main"].into_iter().find(|base| git_ok(&main, &["rev-parse", "-q", "--verify", base])).map(str::to_owned)).ok_or("no origin/main or main to start from")?;
    git_checked(&main, &["worktree", "add", "-q", "--no-track", "-b", slug, &lane.to_string_lossy(), &base])?;
    lane_issue::record(&main, slug, &item)?;
    Ok(item)
}
