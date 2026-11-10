use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use time::OffsetDateTime;

use super::first_commit;
use super::lander::state_home;
use super::queue::{self, RedFix, Ticket, rfc3339};
use super::repositories;
use super::setting;
use north_base::land_ticket;
mod arguments;
mod refuse;
use refuse::{refuse_archived, refuse_conflict, refuse_lane_gates, refuse_stale_revert, refuse_static_gates, refuse_unfetchable};
use super::scans::outgoing;
use super::slots::runner;
use crate::landing::lane_squash::{self, Squash};
use crate::land::scans::outgoing::denylist::Denylist;
use crate::landing_refusals;

const USAGE: &str = "usage: north land enqueue [--repo DIR] [--message FILE | --several-commits REASON] [--keep-lane]";
pub const REFUSALS: &str = "north/enqueue-refusals.jsonl";

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
    pub denylist: &'a Denylist,
}

pub(super) fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    match args.first() {
        Some(&"fetch") => north_git_locks::retry_after_clearing(repo, || run_git(repo, args)),
        _ => run_git(repo, args),
    }
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
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

pub fn is_main_red_marker(line: &str) -> bool {
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

fn is_other_host(lander: &str, host: &str) -> bool {
    !lander.is_empty() && lander.split('.').next() != host.split('.').next()
}

fn lander_ledger_at(lander: &str, queue: &Path, environment: &Environment) -> Option<String> {
    is_other_host(lander, environment.host).then(|| fs::read_to_string(queue.join(queue::LANDER_LEDGER)).ok()).flatten()
}

/// Writes a new ticket: an outboxed one replaces its lane's older outboxed ticket, and one in the lander's own queue is
/// recorded as queued, its older ticket left for the lander to decide as replaced.
fn file(directory: &Path, path: &Path, ticket: &Ticket, outbox: bool) -> Result<(), String> {
    queue::write(path, ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    if outbox {
        replace_older(directory, path, &ticket.root, &ticket.branch).map_err(|error| format!("cannot replace this lane's older ticket in {}: {error}", directory.display()))
    } else {
        land_ticket::queued(path, "enqueued").map_err(|error| format!("cannot record {}: {error}", path.display()))
    }
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

type Marks = (Option<RedFix>, Option<String>, Option<String>);

fn marks(root: &Path, base: &str, head: &str, destination: &str) -> Result<Marks, String> {
    let red_fix = red_fix(root, base, destination)?;
    let urgent = red_fix.is_none().then(|| pays_debt(root, base, head)).flatten();
    Ok((red_fix, super::lander_fix::marked(root, base, head)?, urgent))
}

pub fn enqueue(repo: &Path, shape: Shape, keep_lane: bool, environment: &Environment) -> Result<Queued, String> {
    let (root, branch) = checkout(repo)?;
    let origin = git(&root, &["remote", "get-url", "--push", "origin"]).map_err(|_| "origin has no push URL".to_owned())?;
    repositories::servable(&root).map_err(|error| format!("the lander cannot serve this repository: {error}"))?;
    refuse_archived(&origin)?;
    let destination = default_branch(&root);
    let (remote_ref, base) = fetch_base(&root, &destination)?;
    refuse_conflict(&root, &base, &destination)?;
    super::scans::generated_registry::check(&root, &base, &git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?)?;
    let folded = crate::landing::merge_continue::fold(&root, &root.join(crate::landing::merge_set::DEFAULT_CONFIG), &base)?;
    if !folded.is_empty() {
        println!("safe-push: sorted {} and folded the result into the lane", folded.join(", "));
    }
    let alone_tree = check_lane(&root, &base, &branch, &origin, &destination, environment)?;
    if alone_tree.is_none() {
        println!("safe-push: the private-kind judge has no verdict yet, so the lane waits in the landing queue and the lander judges it again in its alone check before it lands.");
    }
    let first_commit_secs = first_commit::seconds(&root, &branch, &base);
    let (squash, several_commits) = match shape {
        Shape::OneCommit { message } => (lane_squash::squash(&root, &base, message)?, None),
        Shape::SeveralCommits { reason } => (Squash::Kept(format!("--several-commits: {reason}")), Some(reason.to_owned())),
    };
    let head = git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let alone = checked_alone(&root, &head, alone_tree.as_deref(), &base);
    lane_squash::refuse_unfinished_subject(&root, &branch)?;
    let lander_host = setting::landing(&root, &remote_ref, "landing.lander-host");
    let queue = queue::directory(environment.state_home, &origin);
    super::scans::withdrawal::refuse_requeue(&queue, &root, &branch, &head, environment.session)?;
    if let Err((gates, detail)) = refuse_lane_gates(&root, &base, &head, environment.state_home, lander_ledger_at(&lander_host, &queue, environment), &crate::dep_gate::PublicRegistries)? {
        judged(environment, &root, &branch, &head, &gates, Err(detail))?;
    }
    judged(environment, &root, &branch, &head, "revert guard", refuse_stale_revert(&root, &base, &head))?;
    let (red_fix, lander_fix, urgent) = marks(&root, &base, &head, &destination)?;
    let queued_micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    let ticket = Ticket {
        origin,
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
        keep_lane,
        urgent,
        lander_fix,
        published: published(&root, &branch, &destination),
        ..alone
    };
    let outbox = is_other_host(&lander_host, environment.host);
    let directory = if outbox { queue.join(queue::OUTBOX) } else { queue };
    let path = directory.join(queue::file_name(queued_micros, &branch));
    file(&directory, &path, &ticket, outbox)?;
    Ok(Queued { path, outbox, ticket, squash })
}

/// The lane's branch and the root of its checkout.
fn checkout(repo: &Path) -> Result<(PathBuf, String), String> {
    let root = PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"])?);
    let branch = git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|_| "HEAD is detached; switch to the lane's own branch".to_owned())?;
    Ok((root, branch))
}

/// Fetches `destination` into its remote ref and returns the ref and the commit it now names.
fn fetch_base(root: &Path, destination: &str) -> Result<(String, String), String> {
    let remote_ref = format!("refs/remotes/origin/{destination}");
    git(root, &["fetch", "-q", "origin", &format!("+refs/heads/{destination}:{remote_ref}")])
        .map_err(|error| format!("could not fetch origin/{destination}: {error}"))?;
    let base = git(root, &["rev-parse", "--verify", &format!("{remote_ref}^{{commit}}")])?;
    Ok((remote_ref, base))
}

/// `outcome` as it stands at enqueue: a refusal, or under `landing_refusals = "shadow"` a logged would-be refusal.
fn judged(environment: &Environment, root: &Path, branch: &str, head: &str, gate: &str, outcome: Result<(), String>) -> Result<(), String> {
    landing_refusals::Lane { at: "enqueue", root: &root.to_string_lossy(), branch, head }.judged(environment.state_home, gate, outcome)
}

fn checked_alone(root: &Path, head: &str, checked_tree: Option<&str>, base: &str) -> Ticket {
    let passed = checked_tree.is_some_and(|checked| git(root, &["rev-parse", "--verify", &format!("{head}^{{tree}}")]).is_ok_and(|tree| tree == checked));
    Ticket { alone_green: passed.then(|| head.to_owned()), alone_base: passed.then(|| base.to_owned()), ..Ticket::default() }
}

/// The checked tree when every gate judged the lane, or `None` when only the private-kind judge had no verdict yet.
fn check_lane(root: &Path, base: &str, branch: &str, origin: &str, destination: &str, environment: &Environment) -> Result<Option<String>, String> {
    let lane_head = git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    if let Err(gate) = refuse_static_gates(root, base, "HEAD") {
        judged(environment, root, branch, &lane_head, gate, Err(format!("{gate} refused the landing")))?;
    }
    let bodies = git(root, &["log", "--format=%B", &format!("{base}..HEAD")])?;
    judged(environment, root, branch, &lane_head, "commit trailers", super::divergence::refuse_malformed(&bodies))?;
    let gate = outgoing::enqueue_gate(root, base, environment.denylist, origin, destination)?;
    runner::fails_first(root, base, &lane_head)?;
    refuse_unfetchable(root, base, &lane_head)?;
    runner::refuse_queue_checks(root, base, &lane_head)?;
    let tree = git(root, &["rev-parse", "--verify", &format!("{lane_head}^{{tree}}")])?;
    Ok((gate == outgoing::Gate::Judged).then_some(tree))
}

pub fn run(arguments: &[String]) -> u8 {
    if std::env::var_os("NORTH_LAND_SERVING").is_some_and(|value| !value.is_empty()) {
        println!("Done: not queued, this runs inside a lander check and the lander lands its candidate itself");
        return 0;
    }
    let parsed = match arguments::parse(arguments) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let repo = parsed.repo;
    let host = repositories::local_host();
    let session = std::env::var("NORTH_FLEET_SESSION").ok().filter(|name| !name.is_empty());
    let state_home = state_home();
    let sources = Denylist::from_env();
    let environment = Environment { state_home: &state_home, host: &host, session: session.as_deref(), denylist: &sources };
    match enqueue(repo, parsed.shape(), parsed.keep_lane, &environment) {
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
            record_refusal(&state_home, repo, &error);
            eprintln!("safe-push: ✗ cannot queue the lane: {error}; NOT queued.");
            1
        }
    }
}

fn record_refusal(state_home: &Path, repo: &Path, error: &str) {
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros());
    let row = serde_json::json!({ "micros": micros, "root": repo.to_string_lossy(), "error": error });
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(state_home.join(REFUSALS)) {
        let _ = io::Write::write_all(&mut file, format!("{row}\n").as_bytes());
    }
}

#[cfg(test)]
#[path = "enqueue_tests.rs"]
mod tests;
