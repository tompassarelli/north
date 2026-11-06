use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use north_goals::attribute::{first_red, red_contracts};

use super::super::candidate::{self, git};
use super::super::lander::Lander;
use super::super::sheriff;
use crate::landing::after_landing::Failure;

pub struct Landed<'a> {
    pub project: &'a str,
    pub main: &'a Path,
    pub tree: &'a Path,
    pub scratch: &'a Path,
    pub base: &'a str,
    pub head: &'a str,
}

struct Probe<'a> {
    scratch: &'a Path,
    seen: HashMap<String, BTreeSet<String>>,
}

impl Probe<'_> {
    fn red(&mut self, commit: &str) -> BTreeSet<String> {
        if let Some(red) = self.seen.get(commit) {
            return red.clone();
        }
        let red = git(self.scratch, &["checkout", "-q", "--detach", commit]).ok().and_then(|_| red_contracts(self.scratch).ok()).unwrap_or_default();
        self.seen.insert(commit.to_owned(), red.clone());
        red
    }
}

fn settled(tree: &Path) -> Result<BTreeSet<String>, String> {
    let first = red_contracts(tree)?;
    if first.is_empty() {
        return Ok(first);
    }
    Ok(first.intersection(&red_contracts(tree)?).cloned().collect())
}

/// Reverts, through the landing queue, each landed commit that turned a contract goal red: red in the landed tree twice, green at the base of the landing.
pub fn gate(landed: &Landed, link: &dyn Fn(&Path) -> Result<(), String>) -> Vec<Failure> {
    if !north_goals::PROJECTS.contains(&landed.project) {
        return Vec::new();
    }
    let failure = |output: String| vec![Failure { gate: "goals".to_owned(), output }];
    let red = match settled(landed.tree) {
        Ok(red) if red.is_empty() => return Vec::new(),
        Ok(red) => red,
        Err(why) => return failure(format!("goals.toml at {} does not read: {why}", landed.head)),
    };
    let scratch = landed.scratch.to_string_lossy().into_owned();
    let added = git(landed.main, &["worktree", "add", "--quiet", "--detach", &scratch, landed.base]).and_then(|_| link(landed.scratch));
    let found = added.and_then(|()| culprits(landed, red));
    let _ = git(landed.main, &["worktree", "remove", "--force", &scratch]);
    let lander = Lander::new(landed.main, Vec::new(), 1);
    match found.and_then(|found| Ok((found, lander?))) {
        Ok((found, lander)) => found.into_iter().map(|(commit, goals)| revert(landed, &lander, &commit, &goals)).collect(),
        Err(why) => failure(format!("cannot attribute the red contract goals of {}: {why}", landed.head)),
    }
}

fn culprits(landed: &Landed, red: BTreeSet<String>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut probe = Probe { scratch: landed.scratch, seen: HashMap::new() };
    let fresh: Vec<String> = red.difference(&probe.red(landed.base)).cloned().collect();
    let range = format!("{}..{}", landed.base, landed.head);
    let commits: Vec<String> = git(landed.main, &["rev-list", "--reverse", "--first-parent", &range])?.lines().map(str::to_owned).collect();
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for goal in fresh {
        if let Some(at) = first_red(&commits, |commit| probe.red(commit).contains(&goal)) {
            found.entry(commits[at].clone()).or_default().push(goal);
        }
    }
    Ok(found)
}

fn revert(landed: &Landed, lander: &Lander, commit: &str, goals: &[String]) -> Failure {
    let names = goals.join(", ");
    let outcome = queue(landed, lander, commit, &names);
    Failure { gate: format!("goals: {names}"), output: format!("contract goals {names} were green at {} and red at {}; {}", landed.base, landed.head, outcome.unwrap_or_else(|why| format!("{commit} was not reverted: {why}"))) }
}

fn queue(landed: &Landed, lander: &Lander, commit: &str, names: &str) -> Result<String, String> {
    let message = git(landed.main, &["log", "-1", "--format=%B", commit])?;
    if sheriff::is_revert(&message) {
        return Err("it is itself a revert".to_owned());
    }
    let marker = lander.queue.join("goal-reverts").join(commit);
    if marker.exists() {
        return Ok(format!("the revert of {commit} is already queued"));
    }
    let worktree = candidate::lander_worktree(landed.main, "lander-goals")?;
    candidate::ensure(landed.main, &worktree)?;
    if !sheriff::without(&worktree, landed.head, commit)? {
        return Err(format!("it no longer reverts cleanly onto {}", landed.head));
    }
    let branch = format!("goals-revert-{}", &commit[..commit.len().min(12)]);
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = message.lines().next().unwrap_or_default();
    let text = format!("Revert {subject}\n\nContract goals {names} were green before this landing and red at {}, so the after-landing run reverts it through the landing queue.\n\nReverts: {commit}\nGoal-red: {names}", landed.head);
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &text])?;
    sheriff::queue_revert(lander, &worktree, &branch)?;
    let _ = std::fs::create_dir_all(marker.parent().unwrap_or(&lander.queue)).and_then(|()| std::fs::write(&marker, &branch));
    Ok(format!("{commit} is reverted as {branch} through the landing queue"))
}

#[cfg(test)]
#[path = "goals_tests.rs"]
mod tests;
