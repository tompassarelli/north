use std::fs;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::platform::github;

const USAGE: &str = "usage: north after-landing --git-dir DIR --base BASE --head HEAD --safe-push PATH

Run safe-push's after-landing gates and each `after-landing.check = NAME :: COMMAND`
in HEAD's .safe-push on BASE..HEAD in a detached worktree of HEAD, with each `mutant.link` and `after-landing.link` path linked in from the checkout on main, then add a section for HEAD naming every gate that failed to the repository's one open After-landing gate debt issue; a gate already marked
for HEAD is not added again.";
const MARKER: &str = "<!-- after-landing-gate: ";
const TAIL_LINES: usize = 15;
const BODY_LIMIT: usize = 60_000;
const TITLE: &str = "After-landing gate debt";
const HEADER: &str = "Gates that run after landing and failed; fix forward in the next landing of the same lane; each row is one landed commit.";
const DROPPED: &str = "Older sections dropped: ";
const RUNNER: &str = "after-landing-runner";

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
    let mut body = format!("{HEADER}\n");
    if dropped > 0 {
        body.push_str(&format!("{DROPPED}{dropped}.\n"));
    }
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

fn gate_and_file(args: &Args) -> Result<String, String> {
    let main = main_checkout(&args.git_dir)?;
    let repo = repo_slug(&git(&main, &["remote", "get-url", "origin"])?)
        .ok_or_else(|| "origin is not a GitHub repository".to_owned())?;
    let failures = run_gates(&main, args)?;
    if failures.is_empty() {
        return Ok(format!("every gate passed on {}", args.head));
    }
    let existing = filed(&repo)?;
    let date = git(&main, &["log", "-1", "--format=%cs", &args.head])?;
    match filing(&args.head, date.trim(), existing.as_ref(), &failures) {
        Filing::Nothing => Ok(format!("{repo}#{} already names every failed gate", existing.map_or(0, |filed| filed.number))),
        Filing::Edit { number, body } => {
            gh_with_input(&["issue", "edit", &number.to_string(), "-R", &repo, "--body-file", "-"], &body)?;
            Ok(format!("added the new failures to {repo}#{number}"))
        }
        Filing::Create { title, body } => {
            let labels = labels(&repo)?;
            let mut command = vec!["issue", "create", "-R", &repo, "--title", &title, "--body", &body];
            for label in &labels {
                command.extend(["--label", label]);
            }
            Ok(format!("filed {}", gh(&command)?.trim()))
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
    let report = cache.join("north/after-landing").join(format!("{name}.report"));
    let tree_text = tree.to_string_lossy().into_owned();
    git(main, &["worktree", "add", "--quiet", "--detach", &tree_text, &args.head])?;
    let outcome = gates_in(main, &tree, &report, args);
    let removed = git(main, &["worktree", "remove", "--force", &tree_text]);
    let _ = fs::remove_dir_all(&report);
    let failures = outcome?;
    removed?;
    Ok(failures)
}

fn gates_in(main: &Path, tree: &Path, report: &Path, args: &Args) -> Result<Vec<Failure>, String> {
    let mut links = git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "mutant.link"]).unwrap_or_default();
    links.push_str(&git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "after-landing.link"]).unwrap_or_default());
    link_paths(main, tree, &links)?;
    let status = Command::new(&args.safe_push)
        .args(["--after-landing", &args.base, &args.head])
        .arg(report)
        .current_dir(tree)
        .status()
        .map_err(|error| format!("cannot run {}: {error}", args.safe_push.display()))?;
    let mut failures = Vec::new();
    if let Ok(entries) = fs::read_dir(report) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(gate) = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(".log")) {
                failures.push(Failure { gate: gate.to_owned(), output: fs::read_to_string(&path).unwrap_or_default() });
            }
        }
    }
    failures.sort_by(|left, right| left.gate.cmp(&right.gate));
    if !matches!(status.code(), Some(0 | 1)) || (status.code() == Some(1) && failures.is_empty()) {
        failures.push(Failure { gate: RUNNER.to_owned(), output: format!("safe-push --after-landing exited {status}") });
    }
    let checks = git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "after-landing.check"]).unwrap_or_default();
    failures.extend(repo_check_failures(tree, &repo_checks(&checks)));
    Ok(failures)
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

fn filed(repo: &str) -> Result<Option<Filed>, String> {
    let search = format!("\"{TITLE}\" in:title");
    let listed = github::json(&["issue", "list", "-R", repo, "--state", "open", "--search", &search, "--json", "number,title,body"])?;
    Ok(listed.as_array().into_iter().flatten().find(|issue| issue["title"] == TITLE).and_then(|issue| {
        Some(Filed { number: issue["number"].as_u64()?, body: issue["body"].as_str().unwrap_or_default().to_owned() })
    }))
}

fn labels(repo: &str) -> Result<Vec<String>, String> {
    let listed = github::json(&["label", "list", "-R", repo, "--json", "name", "-L", "500"])?;
    let present: Vec<&str> = listed.as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()).collect();
    Ok(["bug", "priority:later", "evidence:code"].into_iter().filter(|label| present.contains(label)).map(str::to_owned).collect())
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

fn gh_with_input(args: &[&str], input: &str) -> Result<(), String> {
    let verb = args[..2].join(" ");
    let mut child = Command::new("gh")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("gh {verb}: {error}"))?;
    child.stdin.take().ok_or_else(|| format!("gh {verb}: stdin is not piped"))?.write_all(input.as_bytes()).map_err(|error| format!("gh {verb}: {error}"))?;
    let output = child.wait_with_output().map_err(|error| format!("gh {verb}: {error}"))?;
    if !output.status.success() {
        return Err(format!("gh {verb}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}

fn gh(args: &[&str]) -> Result<String, String> {
    let output = github::run(args).map_err(|error| format!("gh {}: {error}", args[..2].join(" ")))?;
    if !output.status.success() {
        return Err(format!("gh {}: {}", args[..2].join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "after_landing_tests.rs"]
mod tests;
