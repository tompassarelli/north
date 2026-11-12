use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::issue_body::Ledger;
use crate::platform::rolling_issue::{self, Tracker};

const USAGE: &str = "usage: north after-landing --git-dir DIR --base BASE --head HEAD

Run the after-landing gates (code-health, crate-lines, instruction-limits, generated-rewrite, ci-budget) and each `after-landing.check = NAME :: COMMAND`
in HEAD's .safe-push on BASE..HEAD in a detached worktree of HEAD, with each `mutant.link` and `after-landing.link` path linked in from the checkout on main, then add a section for HEAD naming every gate that failed to the repository's one open After-landing gate debt issue; a gate already marked
for HEAD is not added again.";
mod body;

use body::{dropped_before, kept_boxes, render, split_sections};

const ADVISORY_FROM: &str = "north-after-landing-advisory-base";
const MARKER: &str = "<!-- after-landing-gate: ";
const TAIL_LINES: usize = 15;
const BODY_LIMIT: usize = 60_000;
const TITLE: &str = "After-landing gate debt";

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
    let (mut dropped, kept) = (dropped_before(current), kept_boxes(current));
    while sections.len() > 1 && render(dropped, &kept, &sections).chars().count() > BODY_LIMIT {
        sections.pop();
        dropped += 1;
    }
    let body = render(dropped, &kept, &sections);
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

struct Args {
    git_dir: PathBuf,
    base: String,
    head: String,
}

fn parse(arguments: &[String]) -> Option<Args> {
    let mut found: [Option<&String>; 4] = [None; 4];
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let slot = ["--git-dir", "--base", "--head", "--safe-push"].iter().position(|name| name == flag)?;
        found[slot] = Some(rest.next()?);
    }
    let [Some(git_dir), Some(base), Some(head), _] = found else {
        return None;
    };
    Some(Args { git_dir: git_dir.into(), base: base.clone(), head: head.clone() })
}

pub fn run(arguments: &[String], ledger: &dyn Ledger) -> u8 {
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
    for line in check_boxes(&args).into_iter().chain(tick_boxes(&args, ledger)) {
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

/// Runs each landed `Box:` trailer's own Done-when command through `north task check --done-when`,
/// which ticks a passing box with its Evidence stamp before the landing tick, which leaves such a box to it.
fn check_boxes(args: &Args) -> Vec<String> {
    let (main, north) = match (main_checkout(&args.git_dir), std::env::current_exe()) {
        (Ok(main), Ok(north)) => (main, north),
        (Err(error), _) => return vec![format!("no Done-when command run: {error}")],
        (_, Err(error)) => return vec![format!("no Done-when command run: cannot find the north binary: {error}")],
    };
    let landed = match crate::land::after::tick::landed(&main, &args.base, &args.head) {
        Ok(landed) => landed,
        Err(error) => return vec![format!("no Done-when command run: {error}")],
    };
    landed
        .iter()
        .map(|landed| {
            let issue = landed.issue.to_string();
            match Command::new(&north).args(["task", "check", "--done-when", "--box", &landed.box_text, &issue]).current_dir(&main).output() {
                Ok(output) => format!("Done-when check of {issue} box {:?}: {}{}", landed.box_text, String::from_utf8_lossy(&output.stdout).trim(), String::from_utf8_lossy(&output.stderr).trim()),
                Err(error) => format!("Done-when check of {issue} box {:?}: cannot run {}: {error}", landed.box_text, north.display()),
            }
        })
        .collect()
}

fn tick_boxes(args: &Args, ledger: &dyn Ledger) -> Vec<String> {
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
    lines.extend(crate::land::after::tick::revert_queue(&queue, &main, &args.base, &args.head));
    lines.push(drained(&crate::issue_body::directory(&queue), ledger));
    lines
}

fn drained(outbox: &Path, ledger: &dyn Ledger) -> String {
    let github = crate::issue_body::Gh::new(crate::issue_body::CALL_LIMIT);
    match crate::issue_body::drain(outbox, &github, ledger, crate::issue_body::DRAIN_LIMIT) {
        Ok(drained) => format!(
            "issue bodies: {} written, {} already, {} refused, {} closed, {} waiting{}; {} recorded for the ledger{}{}",
            drained.written,
            drained.already,
            drained.refused,
            drained.closed,
            drained.waiting,
            drained.failure.map_or_else(String::new, |why| format!(" ({why})")),
            drained.recorded,
            if drained.unrecorded.is_empty() { String::new() } else { format!(" ({})", drained.unrecorded.join("; ")) },
            drained.leaves.iter().map(|leaf| format!("; {leaf}")).collect::<String>()
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
    let mut failures = crate::land::after::gates::run(&crate::land::after::gates::Landed { tree, base: &args.base, head: &args.head, code_health: &crate::land::after::gates::code_health_program() });
    let checks = repo_checks(&git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "after-landing.check"]).unwrap_or_default());
    let advisory = repo_checks(&git(tree, &["config", "--blob", "HEAD:.safe-push", "--get-all", "after-landing.advisory"]).unwrap_or_default());
    let judged_from = if advisory.is_empty() { None } else { advisory_base(main, args) };
    let built = if judged_from.is_some() { [checks.as_slice(), advisory.as_slice()].concat() } else { checks.clone() };
    build_elsewhere(tree, args, &built);
    failures.extend(repo_check_failures(tree, &checks, args.base.as_str(), args.head.as_str()));
    match judged_from {
        Some(base) => failures.extend(repo_advisory_failures(tree, &advisory, &base, args.head.as_str())),
        None if !advisory.is_empty() => println!("after-landing: the advisory checks of {} wait for the after-landing run of the newer main, which judges this range with its own", args.head),
        None => {}
    }
    failures.extend(goal_failures(main, tree, args, &links));
    Ok(failures)
}

fn advisory_base(main: &Path, args: &Args) -> Option<String> {
    let deferred = args.git_dir.join(ADVISORY_FROM);
    let ancestor = |older: &str, newer: &str| git(main, &["merge-base", "--is-ancestor", older, newer]).is_ok();
    let recorded = fs::read_to_string(&deferred).ok().map(|base| base.trim().to_owned()).filter(|base| !base.is_empty() && ancestor(base, &args.base));
    let from = recorded.unwrap_or_else(|| args.base.clone());
    let tip = git(main, &["rev-parse", "refs/remotes/origin/main"]).ok().map(|tip| tip.trim().to_owned());
    if tip.is_some_and(|tip| tip != args.head && ancestor(&args.head, &tip)) {
        let _ = fs::write(&deferred, &from);
        return None;
    }
    let _ = fs::remove_file(&deferred);
    Some(from)
}

fn build_elsewhere(tree: &Path, args: &Args, checks: &[(String, String)]) {
    let codes: std::collections::BTreeSet<&str> = checks.iter().filter_map(|(_, command)| head_north_code(command)).collect();
    for code in codes {
        let wanted = north_land_scans::tree_build::Wanted { directory: tree, base: &args.base, head: &args.head, code, caller: "after-landing" };
        north_land_scans::tree_build::prefetch(&wanted, &mut std::io::stdout());
    }
}

fn head_north_code(command: &str) -> Option<&str> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let at = words.iter().position(|word| word.ends_with("bin/head-north"))?;
    let flags = &words[at + 1..];
    if flags.contains(&"--no-build") {
        return None;
    }
    flags.iter().position(|word| *word == "--code").and_then(|code| flags.get(code + 1)).copied()
}

fn goal_failures(main: &Path, tree: &Path, args: &Args, links: &str) -> Vec<Failure> {
    let Some(project) = git(main, &["remote", "get-url", "origin"]).ok().and_then(|url| repo_slug(&url)).and_then(|slug| slug.rsplit('/').next().map(str::to_owned)) else { return Vec::new() };
    let scratch = tree.with_file_name(format!("{}-goals", tree.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
    let landed = crate::land::after::concerns::Landed { project: &project, main, tree, scratch: &scratch, base: &args.base, head: &args.head };
    crate::land::after::concerns::gate(&landed, &|target| link_paths(main, target, links))
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

pub fn repo_check_failures(tree: &Path, checks: &[(String, String)], base: &str, head: &str) -> Vec<Failure> {
    checks
        .iter()
        .filter_map(|(name, command)| {
            let output = match Command::new("sh").args(["-c", command]).current_dir(tree).env("SAFE_PUSH_LAND_BASE", base).env("SAFE_PUSH_LAND_HEAD", head).output() {
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

/// The `after-landing.advisory` checks: whatever a check prints, and whether it exits 0 or not, is debt on the
/// After-landing gate debt issue, never a refusal and never a revert.
pub fn repo_advisory_failures(tree: &Path, checks: &[(String, String)], base: &str, head: &str) -> Vec<Failure> {
    checks
        .iter()
        .filter_map(|(name, command)| {
            let output = match Command::new("sh").args(["-c", command]).current_dir(tree).env("SAFE_PUSH_LAND_BASE", base).env("SAFE_PUSH_LAND_HEAD", head).output() {
                Ok(output) => format!("{}{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr), if output.status.success() { String::new() } else { format!("\n{command} exited {}", output.status) }),
                Err(error) => format!("cannot run {command}: {error}"),
            };
            if output.trim().is_empty() {
                println!("after-landing: advisory {name} found nothing");
                return None;
            }
            println!("after-landing: advisory {name} recorded as debt\n{output}");
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
