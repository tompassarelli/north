use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::github;

const USAGE: &str = "usage: north after-landing --git-dir DIR --base BASE --head HEAD --safe-push PATH

Run safe-push's after-landing gates on BASE..HEAD in a detached worktree of HEAD,
then file one issue for HEAD naming every gate that failed; a gate already filed
for HEAD is not filed again.";
const MARKER: &str = "<!-- after-landing-gate: ";
const TAIL_LINES: usize = 60;
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
    Comment { number: u64, body: String },
}

pub struct Filed {
    pub number: u64,
    pub text: String,
}

pub fn title(head: &str) -> String {
    format!("After-landing gates failed on {}", &head[..head.len().min(12)])
}

pub fn filing(repo: &str, head: &str, existing: Option<&Filed>, failures: &[Failure]) -> Filing {
    let new: Vec<&Failure> = failures
        .iter()
        .filter(|failure| existing.is_none_or(|filed| !filed.text.contains(&marker(&failure.gate))))
        .collect();
    if new.is_empty() {
        return Filing::Nothing;
    }
    let gates: Vec<&str> = new.iter().map(|failure| failure.gate.as_str()).collect();
    let mut body = format!(
        "{repo}@{head} landed on main: it built and its tests passed. These gates run after landing and failed on it: {}. Fix forward in a follow-up commit; nothing was refused.\n",
        gates.join(", ")
    );
    for failure in new {
        let lines: Vec<&str> = failure.output.lines().collect();
        let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
        body.push_str(&format!("\n{}{} -->\n### {}\n\n```\n{tail}\n```\n", MARKER, failure.gate, failure.gate));
    }
    match existing {
        Some(filed) => Filing::Comment { number: filed.number, body },
        None => Filing::Create { title: title(head), body },
    }
}

fn marker(gate: &str) -> String {
    format!("{MARKER}{gate} -->")
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
    let existing = filed(&repo, &args.head)?;
    match filing(&repo, &args.head, existing.as_ref(), &failures) {
        Filing::Nothing => Ok(format!("{repo}#{} already names every failed gate", existing.map_or(0, |filed| filed.number))),
        Filing::Comment { number, body } => {
            gh(&["issue", "comment", &number.to_string(), "-R", &repo, "--body", &body])?;
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
    let links = git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "mutant.link"]).unwrap_or_default();
    for link in links.lines().filter(|link| !link.is_empty()) {
        let source = main.join(link);
        let target = tree.join(link);
        if source.exists() && fs::symlink_metadata(&target).is_err() {
            symlink(&source, &target).map_err(|error| format!("cannot link {link}: {error}"))?;
        }
    }
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
    Ok(failures)
}

fn filed(repo: &str, head: &str) -> Result<Option<Filed>, String> {
    let title = title(head);
    let search = format!("\"{title}\" in:title");
    let listed = github::json(&["issue", "list", "-R", repo, "--state", "all", "--search", &search, "--json", "number,title,body,comments"])?;
    Ok(listed.as_array().into_iter().flatten().find(|issue| issue["title"] == title.as_str()).and_then(|issue| {
        let comments = issue["comments"].as_array().into_iter().flatten().filter_map(|comment| comment["body"].as_str());
        let text = std::iter::once(issue["body"].as_str().unwrap_or_default()).chain(comments).collect::<Vec<_>>().join("\n");
        Some(Filed { number: issue["number"].as_u64()?, text })
    }))
}

fn labels(repo: &str) -> Result<Vec<String>, String> {
    let listed = github::json(&["label", "list", "-R", repo, "--json", "name", "-L", "500"])?;
    let present: Vec<&str> = listed.as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()).collect();
    Ok(["bug", "priority:later", "evidence:code"].into_iter().filter(|label| present.contains(label)).map(str::to_owned).collect())
}

fn main_checkout(git_dir: &Path) -> Result<PathBuf, String> {
    let listing = git(Path::new("/"), &["--git-dir", &git_dir.to_string_lossy(), "worktree", "list", "--porcelain"])?;
    listing
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .ok_or_else(|| format!("{} lists no main worktree", git_dir.display()))
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
