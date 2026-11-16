use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::issue_body::Ledger;
use north_base::platform::rolling_issue::{self, Tracker};

const USAGE: &str = "usage: north after-landing [--only records|gates] --git-dir DIR --base BASE --head HEAD

Run each landed box's Done-when command, tick the landed boxes and drain the issue-body outbox (the records), then run the after-landing gates (code-health, crate-lines, instruction-limits, generated-rewrite, ci-budget) and each `after-landing.check = NAME :: COMMAND`
in HEAD's .safe-push on BASE..HEAD in a detached worktree of HEAD, with each `mutant.link` and `after-landing.link` path linked in from the checkout on main, then file a child issue under the repository's open After-landing gate debt issue for each gate that failed on HEAD; a gate whose child is open already is not filed again. `--only records` or `--only gates` runs one part: the lander's follow keeps the records on the lander's host and runs the gates where `post_landing_host` places them.";

const ADVISORY_FROM: &str = "north-after-landing-advisory-base";
const TAIL_LINES: usize = 15;
const TITLE: &str = "After-landing gate debt";
const FIELDS: &str = "Category: bug-known-cause\nSpec: measured\nScope: cross-module\nClass: a gate that runs after landing failed on a landed commit: the child names its gate and commit.\n";
const STANDING_BODY: &str = "Gates that run after landing and failed. Each failed gate is a child issue of this one, fixed forward on main and closed by its own Done when.";

#[derive(Debug, Eq, PartialEq)]
pub struct Failure {
    pub gate: String,
    pub output: String,
    pub check: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Child {
    pub parent: u64,
    pub title: String,
    pub body: String,
}

/// The children under the standing debt issue `parent` for HEAD's failed gates: one per gate, skipping a gate whose child is open under any commit.
pub fn children(parent: u64, head: &str, date: &str, open_titles: &[String], failures: &[Failure]) -> Vec<Child> {
    let sha = short(head);
    failures
        .iter()
        .filter(|failure| {
            let prefix = format!("{TITLE}: {} failed on ", failure.gate);
            !open_titles.iter().any(|title| title.starts_with(&prefix))
        })
        .map(|failure| Child { parent, title: format!("{TITLE}: {} failed on {sha}", failure.gate), body: child_body(sha, date, failure) })
        .collect()
}

fn short(head: &str) -> &str {
    &head[..head.len().min(12)]
}

fn child_body(sha: &str, date: &str, failure: &Failure) -> String {
    let lines: Vec<&str> = failure.output.lines().collect();
    let tail = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    format!(
        "{FIELDS}\nThe {} gate failed on {sha} ({date}), a commit already landed: fix it forward on main, then close this issue.\n\nDone when: `{}` exits 0.\n\n```text\n{tail}\n```\n",
        failure.gate, failure.check
    )
}

struct Args {
    git_dir: PathBuf,
    base: String,
    head: String,
    only: Option<String>,
}

fn parse(arguments: &[String]) -> Option<Args> {
    let mut found: [Option<&String>; 5] = [None; 5];
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        let slot = ["--git-dir", "--base", "--head", "--safe-push", "--only"].iter().position(|name| name == flag)?;
        found[slot] = Some(rest.next()?);
    }
    let [Some(git_dir), Some(base), Some(head), _, only] = found else {
        return None;
    };
    if only.is_some_and(|only| only != "records" && only != "gates") {
        return None;
    }
    Some(Args { git_dir: git_dir.into(), base: base.clone(), head: head.clone(), only: only.cloned() })
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
    if args.only.as_deref() != Some("gates") {
        for line in check_boxes(&args).into_iter().chain(tick_boxes(&args, ledger)) {
            println!("after-landing: {line}");
        }
    }
    if args.only.as_deref() == Some("records") {
        return 0;
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
    let mut tracker = rolling_issue::Gh::of(&repo);
    let Some(standing) = tracker.open(TITLE)? else {
        tracker.create(TITLE, STANDING_BODY)?;
        return Err(format!("{repo} has no open {TITLE} issue: the coordinator was asked to file it, and the {} failed gate(s) of {} were not filed as children; a rerun of this head files them under it", failures.len(), args.head));
    };
    let open: Vec<String> = tracker.listed(&format!("in:title \"{TITLE}\""))?.into_iter().map(|(_, title)| title).collect();
    let date = git(&main, &["log", "-1", "--format=%cs", &args.head])?;
    let filed = children(standing.number, &args.head, date.trim(), &open, &failures);
    if filed.is_empty() {
        return Ok(format!("{repo}#{} already has an open child for every failed gate", standing.number));
    }
    let (mut created, mut refused) = (Vec::new(), Vec::new());
    for child in &filed {
        match file_child(&repo, child) {
            Ok(line) => created.push(line),
            Err(why) => refused.push(why),
        }
    }
    if refused.is_empty() { Ok(created.join("; ")) } else { Err(refused.join("; ")) }
}

fn file_child(repo: &str, child: &Child) -> Result<String, String> {
    let north = std::env::current_exe().map_err(|error| format!("cannot find the north binary: {error}"))?;
    let parent = format!("{repo}#{}", child.parent);
    let output = Command::new(&north)
        .args(["issue", "child", &parent, "--title", &child.title, "--body", &child.body])
        .output()
        .map_err(|error| format!("cannot run {}: {error}", north.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)).trim().to_owned();
    if output.status.success() { Ok(text) } else { Err(format!("{:?} was not filed under {parent}: {text}", child.title)) }
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
    let north = crate::land::after::concerns::this_north();
    let landed = crate::land::after::concerns::Landed { project: &project, main, tree, scratch: &scratch, base: &args.base, head: &args.head, north: &north };
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
            Some(Failure { gate: name.clone(), output, check: command.clone() })
        })
        .collect()
}

/// The `after-landing.advisory` checks: whatever a check prints on stdout, and any exit other than 0, is debt on the
/// After-landing gate debt issue, never a refusal and never a revert.
pub fn repo_advisory_failures(tree: &Path, checks: &[(String, String)], base: &str, head: &str) -> Vec<Failure> {
    checks
        .iter()
        .filter_map(|(name, command)| {
            let output = match Command::new("sh").args(["-c", command]).current_dir(tree).env("SAFE_PUSH_LAND_BASE", base).env("SAFE_PUSH_LAND_HEAD", head).output() {
                Ok(output) if output.status.success() && output.stdout.trim_ascii().is_empty() => String::new(),
                Ok(output) => format!("{}{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr), if output.status.success() { String::new() } else { format!("\n{command} exited {}", output.status) }),
                Err(error) => format!("cannot run {command}: {error}"),
            };
            if output.is_empty() {
                println!("after-landing: advisory {name} found nothing");
                return None;
            }
            println!("after-landing: advisory {name} recorded as debt\n{output}");
            Some(Failure { gate: name.clone(), output, check: command.clone() })
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
