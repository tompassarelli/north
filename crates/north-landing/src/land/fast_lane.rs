use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use super::after;
use super::candidate::{self, git, output};
use super::checks::{self, Started, Verdict};
use super::lander::Lander;
use super::post_land;
use super::queue::Ticket;
use super::sheriff;

const LANDER_CODE: &str = "crates/north-landing/";
const WORKTREE: &str = "lander-fast-revert";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub directory: String,
    pub leaf: bool,
}

pub struct Facts<'a> {
    pub changed: &'a BTreeSet<String>,
    pub queued: &'a HashSet<String>,
    pub packages: &'a [Package],
    pub red_fix: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Full(String),
    Fast(Vec<String>),
}

fn docs(path: &str) -> bool {
    path.ends_with(".md") || path.starts_with("docs/")
}

fn test(path: &str) -> bool {
    path.starts_with("tests/") || path.contains("/tests/") || path.ends_with("_tests.rs")
}

fn owner<'a>(packages: &'a [Package], path: &str) -> Option<&'a Package> {
    packages.iter().filter(|package| ["src/", "tests/"].iter().any(|kind| path.starts_with(&format!("{}{kind}", package.directory)))).max_by_key(|package| package.directory.len())
}

pub fn route(facts: &Facts) -> Route {
    if facts.red_fix {
        return Route::Full("a red fix proves main green with the full precheck".to_owned());
    }
    if let Some(path) = facts.changed.iter().find(|path| facts.queued.contains(*path)) {
        return Route::Full(format!("{path} is also changed by another queued lane"));
    }
    if let Some(path) = facts.changed.iter().find(|path| path.starts_with(LANDER_CODE)) {
        return Route::Full(format!("{path} is lander code"));
    }
    let mut touched = BTreeSet::new();
    for path in facts.changed.iter().filter(|path| !docs(path)) {
        let Some(package) = owner(facts.packages, path) else {
            return Route::Full(format!("{path} is outside docs, tests and crate sources"));
        };
        if !test(path) && !package.leaf {
            return Route::Full(format!("{path} changes {}, which other crates build on", package.name));
        }
        touched.insert(package.name.clone());
    }
    if touched.len() > 1 {
        return Route::Full(format!("it changes {} crates", touched.len()));
    }
    Route::Fast(touched.into_iter().collect())
}

fn quoted(line: &str) -> Vec<String> {
    line.split('"').skip(1).step_by(2).map(str::to_owned).collect()
}

fn package_name(manifest: &str) -> Option<String> {
    let package = manifest.split("[package]").nth(1)?;
    package.lines().find_map(|line| line.trim().strip_prefix("name").map(str::trim).and_then(|rest| rest.strip_prefix('=')).and_then(|value| quoted(value).into_iter().next()))
}

fn depends_on(manifest: &str, name: &str) -> bool {
    manifest.lines().map(str::trim).any(|line| line.strip_prefix(name).is_some_and(|rest| rest.starts_with([' ', '=', '.'])))
}

pub fn packages(worktree: &Path) -> Vec<Package> {
    let Ok(root) = fs::read_to_string(worktree.join("Cargo.toml")) else { return Vec::new() };
    let members = root.split("members").nth(1).and_then(|rest| rest.split_once(']')).map(|(list, _)| quoted(list)).unwrap_or_default();
    let mut manifests: Vec<(String, String)> = members.iter().filter_map(|member| fs::read_to_string(worktree.join(member).join("Cargo.toml")).ok().map(|text| (format!("{}/", member.trim_end_matches('/')), text))).collect();
    if root.contains("[package]") {
        manifests.push((String::new(), root));
    }
    let named: Vec<(String, String, String)> = manifests.into_iter().filter_map(|(directory, text)| package_name(&text).map(|name| (name, directory, text))).collect();
    named.iter().map(|(name, directory, _)| Package { name: name.clone(), directory: directory.clone(), leaf: !named.iter().any(|(other, _, text)| other != name && depends_on(text, name)) }).collect()
}

pub fn changed(repo: &Path, base: &str, head: &str) -> Option<BTreeSet<String>> {
    git(repo, &["diff", "--no-renames", "--name-only", &format!("{base}...{head}")]).ok().map(|text| text.lines().map(str::to_owned).collect())
}

pub fn check(worktree: &Path, crates: &[String], log: &Path) -> Result<Option<Started>, Verdict> {
    if crates.is_empty() {
        return Ok(None);
    }
    let mut argv: Vec<String> = ["cargo", "check", "--locked", "--offline", "--tests"].map(str::to_owned).to_vec();
    for name in crates {
        argv.extend(["-p".to_owned(), name.clone()]);
    }
    checks::start_judged_by_exit(&argv, worktree, log).map(Some)
}

pub struct Following {
    ticket: Ticket,
    base: String,
    head: String,
    run: JoinHandle<Verdict>,
}

pub fn follow(lander: &Lander, ticket: &Ticket, base: &str, head: &str) -> Following {
    let run = post_land::start(lander, base, head, vec![ticket.branch.clone()]);
    Following { ticket: ticket.clone(), base: base.to_owned(), head: head.to_owned(), run }
}

pub fn settle(lander: &Lander, following: &mut Vec<Following>) -> bool {
    let mut settled = false;
    let mut index = 0;
    while index < following.len() {
        if !following[index].run.is_finished() {
            index += 1;
            continue;
        }
        let Following { ticket, base, head, run } = following.swap_remove(index);
        settled = true;
        let verdict = run.join().unwrap_or_else(|_| Verdict::Transient("the post-land thread panicked".to_owned()));
        if let Verdict::RedMain(detail) = verdict {
            match revert(lander, &Landed { ticket: &ticket, base: &base, head: &head }, &detail) {
                Ok(line) => println!("north land: fast lane: {line}"),
                Err(error) => {
                    let message = format!("north land: fast lane: the post-land checks of {} failed ({detail}) and its revert could not be queued: {error}", ticket.branch);
                    println!("{message}");
                    after::send("proxy", &message);
                }
            }
        }
    }
    settled
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

struct Landed<'a> {
    ticket: &'a Ticket,
    base: &'a str,
    head: &'a str,
}

fn revert(lander: &Lander, landed: &Landed, detail: &str) -> Result<String, String> {
    let worktree: PathBuf = candidate::lander_worktree(&lander.repo, WORKTREE)?;
    let main = candidate::prepare(&lander.repo, &worktree)?;
    git(&worktree, &["reset", "-q", "--hard"])?;
    git(&worktree, &["checkout", "-q", "--detach", &main])?;
    let range = format!("{}..{}", landed.base, landed.head);
    let commits = git(&worktree, &["rev-list", "--reverse", &range])?;
    if !output(&worktree, &["revert", "--no-commit", &range])?.status.success() {
        let _ = output(&worktree, &["revert", "--abort"]);
        git(&worktree, &["reset", "-q", "--hard", &main])?;
        return Err(format!("{range} no longer reverts cleanly onto main {}", short(&main)));
    }
    let branch = format!("fast-revert-{}", short(landed.head));
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = git(&worktree, &["log", "-1", "--format=%s", landed.head])?;
    let trailers: Vec<String> = commits.lines().map(|commit| format!("Reverts: {commit}")).collect();
    let message = format!("Revert {subject}\n\n{} landed through the fast lane and its post-land checks failed on {} ({detail}), so the lander reverts it through the landing queue.\n\n{}", landed.ticket.branch, short(landed.head), trailers.join("\n"));
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = sheriff::queue_revert(lander, &worktree, &branch)?;
    if let Some(session) = landed.ticket.session.as_deref().filter(|session| !session.is_empty()) {
        let text = format!("north land: {} landed through the fast lane, but its post-land checks failed on {} ({detail}); the lander queued its revert {branch} ahead of the queue; fix the lane and queue it again", landed.ticket.branch, short(landed.head));
        if !after::send(session, &text) {
            println!("north land: fast lane: could not tell session {session} that {} was reverted", landed.ticket.branch);
        }
    }
    Ok(format!("revert of {} {head} queued as {branch}", landed.ticket.branch))
}

#[cfg(test)]
#[path = "fast_lane_tests.rs"]
mod tests;
