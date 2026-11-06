use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::rolling_issue::{self, Tracker};

const USAGE: &str = "usage: north after-landing --git-dir DIR --base BASE --head HEAD --safe-push PATH

Run the after-landing gates (code-health, crate-lines, instruction-limits, generated-rewrite, ci-budget) and each `after-landing.check = NAME :: COMMAND`
in HEAD's .safe-push on BASE..HEAD in a detached worktree of HEAD, with each `mutant.link` and `after-landing.link` path linked in from the checkout on main, then add a section for HEAD naming every gate that failed to the repository's one open After-landing gate debt issue; a gate already marked
for HEAD is not added again.";
const MARKER: &str = "<!-- after-landing-gate: ";
const TAIL_LINES: usize = 15;
const BODY_LIMIT: usize = 60_000;
const TITLE: &str = "After-landing gate debt";
const FIELDS: &str = "Category: bug-known-cause\nClass: a gate that runs after landing failed on a landed commit: every section below.\n";
const HEADER: &str = "Gates that run after landing and failed; fix forward in the next landing of the same lane; each row is one landed commit.";
const DROPPED: &str = "Older sections dropped: ";
const DONE_WHEN: &str = "\n## Done when\n- [ ] No section remains below: each failed gate is fixed forward on main and its section deleted.\n";

#[derive(Debug, Eq, PartialEq)]
pub struct Failure {
    pub gate: String,
    pub output: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Filing {
    Nothing,
    Create { title: String, body: String },
    Edit { number: u64, body: String },
}

pub struct Filed {
    pub number: u64,
    pub body: String,
}

pub fn filing(head: &str, date: &str, existing: Option<&Filed>, failures: &[Failure]) -> Filing {
    let sha = short(head);
    let current = existing.map_or("", |filed| filed.body.as_str());
    let new: Vec<&Failure> = failures.iter().filter(|failure| !current.contains(&marker(sha, &failure.gate))).collect();
    if new.is_empty() {
        return Filing::Nothing;
    }
    let mut sections = split_sections(current);
    sections.insert(0, section(sha, date, &new));
    let mut dropped = dropped_before(current);
    while sections.len() > 1 && render(dropped, &sections).chars().count() > BODY_LIMIT {
        sections.pop();
        dropped += 1;
    }
    let body = render(dropped, &sections);
    match existing {
        Some(filed) => Filing::Edit { number: filed.number, body },
        None => Filing::Create { title: TITLE.to_owned(), body },
    }
}

fn short(head: &str) -> &str {
    &head[..head.len().min(12)]
}

fn marker(sha: &str, gate: &str) -> String {
    format!("{MARKER}{sha} {gate} -->")
}

fn section(sha: &str, date: &str, failures: &[&Failure]) -> String {
    let gates: Vec<&str> = failures.iter().map(|failure| failure.gate.as_str()).collect();
    let mut text = String::new();
    for failure in failures {
        text.push_str(&marker(sha, &failure.gate));
        text.push('\n');
    }
    text.push_str(&format!("### {sha} ({date}) gates: {}\n", gates.join(", ")));
    for failure in failures {
        let lines: Vec<&str> = failure.output.lines().collect();
        let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
        text.push_str(&format!("\n{}\n```\n{tail}\n```\n", failure.gate));
    }
    text
}

fn split_sections(body: &str) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    let mut after_marker = false;
    for line in body.lines() {
        let is_marker = line.starts_with(MARKER);
        if is_marker && !after_marker {
            sections.push(String::new());
        }
        if let Some(last) = sections.last_mut() {
            last.push_str(line);
            last.push('\n');
        }
        after_marker = is_marker;
    }
    sections
}

fn dropped_before(body: &str) -> usize {
    body.lines().find_map(|line| line.strip_prefix(DROPPED)?.trim_end_matches('.').parse().ok()).unwrap_or(0)
}

fn render(dropped: usize, sections: &[String]) -> String {
    let mut body = format!("{FIELDS}{HEADER}\n");
    if dropped > 0 {
        body.push_str(&format!("{DROPPED}{dropped}.\n"));
    }
    body.push_str(DONE_WHEN);
    for section in sections {
        body.push('\n');
        body.push_str(section);
    }
    body
}

struct Args {
    git_dir: PathBuf,
    base: String,
    head: String,
    safe_push: PathBuf,
}

fn parse(arguments: &[String]) -> Option<Args> {
    let mut found: [Option<&String>; 4] = [None; 4];
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let slot = ["--git-dir", "--base", "--head", "--safe-push"].iter().position(|name| name == flag)?;
        found[slot] = Some(rest.next()?);
    }
    let [Some(git_dir), Some(base), Some(head), Some(safe_push)] = found else {
        return None;
    };
    Some(Args { git_dir: git_dir.into(), base: base.clone(), head: head.clone(), safe_push: safe_push.into() })
}

pub fn run(arguments: &[String]) -> u8 {
    if let [flag] = arguments
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return 0;
    }
    let Some(args) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    for line in tick_boxes(&args) {
        println!("after-landing: {line}");
    }
    match gate_and_file(&args) {
        Ok(line) => {
            println!("after-landing: {line}");
            0
        }
        Err(error) => {
            eprintln!("after-landing: {error}");
            1
        }
    }
}

fn tick_boxes(args: &Args) -> Vec<String> {
    let main = match main_checkout(&args.git_dir) {
        Ok(main) => main,
        Err(error) => return vec![format!("no box ticked: {error}")],
    };
    let origin = match git(&main, &["remote", "get-url", "--push", "origin"]) {
        Ok(origin) => origin.trim().to_owned(),
        Err(error) => return vec![format!("no box ticked: {error}")],
    };
    let queue = crate::land::queue::directory(&crate::land::lander::state_home(), &origin);
    let mut lines = crate::land::after::tick::queue(&queue, &main, &args.base, &args.head);
    lines.push(drained(&crate::issue_body::directory(&queue)));
    lines
}

fn drained(outbox: &Path) -> String {
    let github = crate::issue_body::Gh::new(crate::issue_body::CALL_LIMIT);
    match crate::issue_body::drain(outbox, &github, crate::issue_body::DRAIN_LIMIT) {
        Ok(drained) => format!(
            "issue bodies: {} written, {} already, {} refused, {} closed, {} waiting{}",
            drained.written,
            drained.already,
            drained.refused,
            drained.closed,
            drained.waiting,
            drained.failure.map_or_else(String::new, |why| format!(" ({why})"))
        ),
        Err(error) => format!("issue bodies: cannot drain {}: {error}", outbox.display()),
    }
}

fn gate_and_file(args: &Args) -> Result<String, String> {
    let main = main_checkout(&args.git_dir)?;
    let repo = repo_slug(&git(&main, &["remote", "get-url", "origin"])?)
        .ok_or_else(|| "origin is not a GitHub repository".to_owned())?;
    let failures = run_gates(&main, args)?;
    if failures.is_empty() {
        return Ok(format!("every gate passed on {}", args.head));
    }
    let mut tracker = rolling_issue::Gh::of(&repo, &["bug", "priority:later", "evidence:code"]);
    let existing = tracker.open(TITLE)?.map(|issue| Filed { number: issue.number, body: issue.body });
    let date = git(&main, &["log", "-1", "--format=%cs", &args.head])?;
    match filing(&args.head, date.trim(), existing.as_ref(), &failures) {
        Filing::Nothing => Ok(format!("{repo}#{} already names every failed gate", existing.map_or(0, |filed| filed.number))),
        Filing::Edit { number, body } => {
            tracker.edit(number, &body)?;
            Ok(format!("added the new failures to {repo}#{number}"))
        }
        Filing::Create { title, body } => {
            tracker.create(&title, &body)?;
            Ok(format!("filed {title} in {repo}"))
        }
    }
}

fn run_gates(main: &Path, args: &Args) -> Result<Vec<Failure>, String> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| "neither XDG_CACHE_HOME nor HOME is set".to_owned())?;
    let name = format!("{}-{}", &args.head[..args.head.len().min(12)], std::process::id());
    let tree = cache.join("north/after-landing").join(&name);
    let tree_text = tree.to_string_lossy().into_owned();
    git(main, &["worktree", "add", "--quiet", "--detach", &tree_text, &args.head])?;
    let outcome = gates_in(main, &tree, args);
    let removed = git(main, &["worktree", "remove", "--force", &tree_text]);
    let failures = outcome?;
    removed?;
    Ok(failures)
}

fn gates_in(main: &Path, tree: &Path, args: &Args) -> Result<Vec<Failure>, String> {
    let mut links = git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "mutant.link"]).unwrap_or_default();
    links.push_str(&git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "after-landing.link"]).unwrap_or_default());
    link_paths(main, tree, &links)?;
    let mut failures = crate::land::after::gates::run(&crate::land::after::gates::Landed { tree, base: &args.base, head: &args.head, safe_push: &args.safe_push });
    let checks = git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "after-landing.check"]).unwrap_or_default();
    failures.extend(repo_check_failures(tree, &repo_checks(&checks)));
    failures.extend(goal_failures(main, tree, args, &links));
    Ok(failures)
}

fn goal_failures(main: &Path, tree: &Path, args: &Args, links: &str) -> Vec<Failure> {
    let Some(project) = git(main, &["remote", "get-url", "origin"]).ok().and_then(|url| repo_slug(&url)).and_then(|slug| slug.rsplit('/').next().map(str::to_owned)) else { return Vec::new() };
    let scratch = tree.with_file_name(format!("{}-goals", tree.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
    let landed = crate::land::after::goals::Landed { project: &project, main, tree, scratch: &scratch, base: &args.base, head: &args.head };
    crate::land::after::goals::gate(&landed, &|target| link_paths(main, target, links))
}

fn link_paths(main: &Path, tree: &Path, links: &str) -> Result<(), String> {
    for link in links.lines().map(str::trim).filter(|link| !link.is_empty()) {
        let source = main.join(link);
        let target = tree.join(link);
        if source.exists() && fs::symlink_metadata(&target).is_err() {
            symlink(&source, &target).map_err(|error| format!("cannot link {link}: {error}"))?;
        }
    }
    Ok(())
}

pub fn repo_checks(config: &str) -> Vec<(String, String)> {
    config
        .lines()
        .filter_map(|line| line.split_once(" :: "))
        .map(|(name, command)| (name.trim().to_owned(), command.trim().to_owned()))
        .filter(|(name, command)| !name.is_empty() && !command.is_empty())
        .collect()
}

pub fn repo_check_failures(tree: &Path, checks: &[(String, String)]) -> Vec<Failure> {
    checks
        .iter()
        .filter_map(|(name, command)| {
            let output = match Command::new("sh").args(["-c", command]).current_dir(tree).output() {
                Ok(output) if output.status.success() => {
                    println!("after-landing: check {name} passed");
                    return None;
                }
                Ok(output) => format!("{}{}\n{command} exited {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr), output.status),
                Err(error) => format!("cannot run {command}: {error}"),
            };
            println!("after-landing: check {name} failed\n{output}");
            Some(Failure { gate: name.clone(), output })
        })
        .collect()
}

fn main_checkout(git_dir: &Path) -> Result<PathBuf, String> {
    let listing = git(Path::new("/"), &["--git-dir", &git_dir.to_string_lossy(), "worktree", "list", "--porcelain"])?;
    main_worktree(&listing).ok_or_else(|| format!("{} lists no main worktree", git_dir.display()))
}

pub fn main_worktree(listing: &str) -> Option<PathBuf> {
    let mut first = None;
    for block in listing.split("\n\n") {
        let path = block.lines().find_map(|line| line.strip_prefix("worktree ")).map(PathBuf::from);
        if first.is_none() {
            first.clone_from(&path);
        }
        if block.lines().any(|line| line == "branch refs/heads/main") {
            return path;
        }
    }
    first
}

pub fn repo_slug(url: &str) -> Option<String> {
    let path = url.trim().trim_end_matches(".git").rsplit_once("github.com")?.1;
    let slug = path.trim_start_matches([':', '/']);
    (slug.split('/').count() == 2).then(|| slug.to_owned())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "after_landing_tests.rs"]
mod tests;
