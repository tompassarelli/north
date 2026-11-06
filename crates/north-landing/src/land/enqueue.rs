use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use time::OffsetDateTime;

use super::check_coverage;
use super::first_commit;
use super::lander::state_home;
use super::queue::{self, RedFix, Ticket};
use super::repositories;
use super::setting;
use super::scans::git::merge_base;
use super::scans::outgoing;
use super::slots::runner;
use crate::landing::lane_squash::{self, Squash};
use crate::land::scans::outgoing::denylist::Denylist;
use crate::watch::facts::slug_of;

const USAGE: &str = "usage: north land enqueue [--repo DIR] [--message FILE | --several-commits REASON] [--keep-lane]";

#[derive(Clone, Copy, Debug)]
pub enum Shape<'a> {
    OneCommit { message: Option<&'a str> },
    SeveralCommits { reason: &'a str },
}

#[derive(Debug)]
pub struct Queued {
    pub path: PathBuf,
    pub outbox: bool,
    pub ticket: Ticket,
    pub squash: Squash,
}

pub struct Environment<'a> {
    pub state_home: &'a Path,
    pub host: &'a str,
    pub session: Option<&'a str>,
    pub south: &'a Path,
    pub denylist: Option<&'a Path>,
}

pub(super) fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn default_branch(repo: &Path) -> String {
    git(repo, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
        .ok()
        .and_then(|reference| reference.strip_prefix("refs/remotes/origin/").map(str::to_owned))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "main".to_owned())
}

pub(super) fn rfc3339(now: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

pub(crate) fn is_main_red_marker(line: &str) -> bool {
    line.strip_prefix("Main-red:").is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
}

pub fn issue_numbers(text: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('#') {
        rest = &rest[at + 1..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse() {
            numbers.push(number);
        }
    }
    numbers
}

#[derive(Deserialize)]
struct OpenIssue {
    number: u64,
    title: String,
}

pub fn pick_issue(listing: &str, destination: &str, named: &[u64]) -> Option<RedFix> {
    let title = format!("{destination} is red");
    let issues: Vec<OpenIssue> = serde_json::from_str(listing).ok()?;
    let issue = issues.into_iter().find(|issue| issue.title == title)?;
    named.contains(&issue.number).then_some(RedFix::Issue(issue.number))
}

fn open_issues(repo: &Path, destination: &str) -> Option<String> {
    let output = Command::new("timeout")
        .current_dir(repo)
        .args(["10", "gh", "issue", "list", "--state", "open", "--search"])
        .arg(format!("\"{destination} is red\" in:title"))
        .args(["--json", "number,title"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn red_fix(repo: &Path, base: &str, destination: &str) -> Result<Option<RedFix>, String> {
    let range = format!("{base}..HEAD");
    let bodies = git(repo, &["log", "--format=%B", &range])?;
    if bodies.lines().any(is_main_red_marker) {
        return Ok(Some(RedFix::Marker));
    }
    let subjects = git(repo, &["log", "--format=%s", &range])?;
    let folded: Vec<&str> = bodies.lines().filter(|line| line.starts_with("- ")).collect();
    let named = issue_numbers(&format!("{subjects}\n{}", folded.join("\n")));
    if named.is_empty() {
        return Ok(None);
    }
    Ok(open_issues(repo, destination).and_then(|listing| pick_issue(&listing, destination, &named)))
}

fn off_lander_host(root: &Path, remote_ref: &str, host: &str) -> bool {
    let lander = setting::landing(root, remote_ref, "landing.lander-host");
    !lander.is_empty() && lander.split('.').next() != host.split('.').next()
}

fn replace_older(directory: &Path, kept: &Path, root: &Path, branch: &str) -> io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        let undecided = path.extension().is_some_and(|extension| extension == "json") && path.is_file() && path != kept;
        if undecided && queue::read(&path).is_ok_and(|older| older.root == root && older.branch == branch) {
            match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
    }
    Ok(())
}

fn refuse_archived(origin: &str) -> Result<(), String> {
    let Some(slug) = origin.contains("github.com").then(|| slug_of(origin)).flatten() else { return Ok(()) };
    let answer = Command::new("timeout").args(["10", "gh", "api", &format!("repos/{slug}"), "--jq", ".archived"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    match answer {
        Ok(output) if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" => Err(format!("GitHub reports {slug} as archived, so origin refuses every push; unarchive it or land elsewhere")),
        _ => Ok(()),
    }
}

fn refuse_static_gates(root: &Path, destination: &str, head: &str) -> Result<(), String> {
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

fn published(root: &Path, branch: &str, destination: &str) -> Option<String> {
    let upstream = git(root, &["rev-parse", "--symbolic-full-name", &format!("{branch}@{{upstream}}")]).ok()?;
    let name = upstream.strip_prefix("refs/remotes/origin/")?;
    (name != destination && name != branch).then(|| name.to_owned())
}

/// Why the lane goes first: it touches a path that pays a debt goal reading over its ceiling in the base'"'"'s goals.toml.
fn pays_debt(root: &Path, base: &str, head: &str) -> Option<String> {
    let registry = git(root, &["show", &format!("{base}:{}", north_goals::registry::PATH)]).ok()?;
    let changed: Vec<String> = git(root, &["diff", "--name-only", &format!("{base}..{head}")]).ok()?.lines().map(str::to_owned).collect();
    north_goals::ceiling::paid(root, &registry, &changed).ok()?.into_iter().next()
}

pub fn enqueue(repo: &Path, shape: Shape, keep_lane: bool, environment: &Environment) -> Result<Queued, String> {
    let root = PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"])?);
    let branch = git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|_| "HEAD is detached; switch to the lane's own branch".to_owned())?;
    let origin = git(&root, &["remote", "get-url", "--push", "origin"]).map_err(|_| "origin has no push URL".to_owned())?;
    repositories::servable(&root).map_err(|error| format!("the lander cannot serve this repository: {error}"))?;
    refuse_archived(&origin)?;
    let destination = default_branch(&root);
    let remote_ref = format!("refs/remotes/origin/{destination}");
    git(&root, &["fetch", "-q", "origin", &format!("+refs/heads/{destination}:{remote_ref}")])
        .map_err(|error| format!("could not fetch origin/{destination}: {error}"))?;
    let base = git(&root, &["rev-parse", "--verify", &format!("{remote_ref}^{{commit}}")])?;
    refuse_static_gates(&root, &base, "HEAD")?;
    super::divergence::refuse_malformed(&git(&root, &["log", "--format=%B", &format!("{base}..HEAD")])?)?;
    check_coverage::refuse_lost_coverage(&root, &base, "HEAD")?;
    let denylist = Denylist { south: environment.south.to_path_buf(), config: environment.denylist.map(Path::to_path_buf) };
    outgoing::enqueue_gate(&root, &base, &denylist, &origin, &destination)?;
    let lane_head = git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    runner::fails_first(&root, &base, &lane_head)?;
    let unchecked = runner::refuse_queue_checks(&root, &base, &lane_head)?;
    let first_commit_secs = first_commit::seconds(&root, &branch, &base);
    let (squash, several_commits) = match shape {
        Shape::OneCommit { message } => (lane_squash::squash(&root, &base, message)?, None),
        Shape::SeveralCommits { reason } => (Squash::Kept(format!("--several-commits: {reason}")), Some(reason.to_owned())),
    };
    let head = git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let red_fix = red_fix(&root, &base, &destination)?;
    let urgent = if red_fix.is_some() { None } else { pays_debt(&root, &base, &head) };
    let queued_micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    let ticket = Ticket {
        origin: origin.clone(),
        root: root.clone(),
        host: environment.host.to_owned(),
        branch: branch.clone(),
        head,
        session: environment.session.map(str::to_owned),
        queued_at: rfc3339(OffsetDateTime::now_utc()),
        queued_micros: Some(queued_micros),
        first_commit_secs,
        several_commits,
        red_fix,
        verdict: None,
        keep_lane,
        urgent,
        decided_on: None,
        published: published(&root, &branch, &destination),
        unchecked,
    };
    let outbox = off_lander_host(&root, &remote_ref, environment.host);
    let queue = queue::directory(environment.state_home, &origin);
    let directory = if outbox { queue.join(queue::OUTBOX) } else { queue };
    let path = directory.join(queue::file_name(queued_micros, &branch));
    queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    replace_older(&directory, &path, &root, &branch).map_err(|error| format!("cannot replace this lane's older ticket in {}: {error}", directory.display()))?;
    Ok(Queued { path, outbox, ticket, squash })
}

pub fn run(arguments: &[String]) -> u8 {
    if std::env::var_os("NORTH_LAND_SERVING").is_some_and(|value| !value.is_empty()) {
        println!("Done: not queued, this runs inside a lander check and the lander lands its candidate itself");
        return 0;
    }
    if let [flag] = arguments
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return 0;
    }
    let (repo, rest) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (Path::new(dir), rest),
        rest => (Path::new("."), rest),
    };
    let (supplied, rest) = match rest {
        [flag, file, rest @ ..] if flag == "--message" => match fs::read_to_string(file) {
            Ok(text) if !text.trim().is_empty() => (Some(text), rest),
            Ok(_) => {
                eprintln!("safe-push: ✗ --message {file} is empty; NOT queued.");
                return 1;
            }
            Err(error) => {
                eprintln!("safe-push: ✗ cannot read --message {file}: {error}; NOT queued.");
                return 1;
            }
        },
        rest => (None, rest),
    };
    let (several_commits, rest) = match rest {
        [flag, reason, rest @ ..] if flag == "--several-commits" && supplied.is_none() && !reason.trim().is_empty() => (Some(reason.as_str()), rest),
        rest => (None, rest),
    };
    let shape = match several_commits {
        Some(reason) => Shape::SeveralCommits { reason },
        None => Shape::OneCommit { message: supplied.as_deref() },
    };
    let (keep_lane, rest) = match rest {
        [flag, rest @ ..] if flag == "--keep-lane" => (true, rest),
        rest => (false, rest),
    };
    if !rest.is_empty() {
        eprintln!("{USAGE}");
        return 64;
    }
    let host = repositories::local_host();
    let session = std::env::var("NORTH_FLEET_SESSION").ok().filter(|name| !name.is_empty());
    let state_home = state_home();
    let sources = Denylist::from_env();
    let environment = Environment { state_home: &state_home, host: &host, session: session.as_deref(), south: &sources.south, denylist: sources.config.as_deref() };
    match enqueue(repo, shape, keep_lane, &environment) {
        Ok(queued) => {
            match &queued.squash {
                Squash::Kept(reason) => println!("safe-push: lane not squashed: {reason}"),
                Squash::Squashed { commits, head } => println!("safe-push: queued the lane's {commits} unpublished commits as one commit {head}"),
                Squash::Unchanged => {}
            }
            println!("Done: queued {}{} {} {}", if queued.outbox { "(outbox) " } else { "" }, queued.ticket.root.display(), queued.ticket.branch, queued.ticket.head);
            0
        }
        Err(error) => {
            eprintln!("safe-push: ✗ cannot queue the lane: {error}; NOT queued.");
            1
        }
    }
}

#[cfg(test)]
#[path = "enqueue_tests.rs"]
mod tests;
