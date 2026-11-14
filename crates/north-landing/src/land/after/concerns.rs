use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use north_concerns::attribute::{first_red, red_contracts};

use super::super::candidate::{self, git};
use super::super::lander::{Lander, state_home};
use super::super::sheriff;
use crate::landing::after_landing::Failure;

const GATE: &str = "concern-revert";
const GATES: &str = "harness/gates.toml";

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

/// Reverts, through the landing queue, each landed commit that turned a contract concern red: red in the landed tree twice, green at the base of the landing; while the `concern-revert` gate is in shadow it logs the revert it would make instead.
pub fn gate(landed: &Landed, link: &dyn Fn(&Path) -> Result<(), String>) -> Vec<Failure> {
    if !north_concerns::PROJECTS.contains(&landed.project) {
        return Vec::new();
    }
    let failure = |output: String| vec![Failure { gate: "concerns".to_owned(), output }];
    let red = match settled(landed.tree) {
        Ok(red) if red.is_empty() => return Vec::new(),
        Ok(red) => red,
        Err(why) => return failure(format!("concerns.toml at {} does not read: {why}", landed.head)),
    };
    let scratch = landed.scratch.to_string_lossy().into_owned();
    let added = git(landed.main, &["worktree", "add", "--quiet", "--detach", &scratch, landed.base]).and_then(|_| link(landed.scratch));
    let found = added.and_then(|()| culprits(landed, red));
    let _ = git(landed.main, &["worktree", "remove", "--force", &scratch]);
    let lander = Lander::new(landed.main, Vec::new());
    match found.and_then(|found| Ok((found, lander?))) {
        Ok((found, _)) if !reverts(landed.tree) => {
            found.iter().for_each(|(commit, concerns)| would_revert(landed, commit, concerns));
            Vec::new()
        }
        Ok((found, lander)) => found.into_iter().map(|(commit, concerns)| revert(landed, &lander, &commit, &concerns)).collect(),
        Err(why) => failure(format!("cannot attribute the red contract concerns of {}: {why}", landed.head)),
    }
}

/// Whether the `concern-revert` gate of the landed tree's harness/gates.toml enforces, read from the tree so a mode change takes effect at the next landing rather than the next promotion.
fn reverts(tree: &Path) -> bool {
    let document = std::fs::read_to_string(tree.join(GATES)).ok().and_then(|text| text.parse::<toml_edit::DocumentMut>().ok());
    document.is_some_and(|document| document["gate"][GATE]["mode"].as_str() == Some("enforce"))
}

fn would_revert(landed: &Landed, commit: &str, concerns: &[String]) {
    println!("north land: contract concerns {} were green at {} and red at {}; the concern-revert gate is in shadow, so {commit} is not reverted", concerns.join(", "), landed.base, landed.head);
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": GATE, "at": at, "project": landed.project, "base": landed.base, "head": landed.head, "would_revert": commit, "concerns": concerns});
    let gates = state_home().join("north/gates");
    let written = std::fs::create_dir_all(&gates).and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(gates.join(format!("{GATE}.jsonl")))).and_then(|mut file| std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes()));
    if let Err(error) = written {
        eprintln!("north land: could not log the concern revert of {commit}: {error}");
    }
}

fn culprits(landed: &Landed, red: BTreeSet<String>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut probe = Probe { scratch: landed.scratch, seen: HashMap::new() };
    let fresh: Vec<String> = red.difference(&probe.red(landed.base)).cloned().collect();
    let range = format!("{}..{}", landed.base, landed.head);
    let commits: Vec<String> = git(landed.main, &["rev-list", "--reverse", "--first-parent", &range])?.lines().map(str::to_owned).collect();
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for concern in fresh {
        if let Some(at) = first_red(&commits, |commit| probe.red(commit).contains(&concern)) {
            found.entry(commits[at].clone()).or_default().push(concern);
        }
    }
    Ok(found)
}

fn revert(landed: &Landed, lander: &Lander, commit: &str, concerns: &[String]) -> Failure {
    let names = concerns.join(", ");
    let outcome = queue(landed, lander, commit, &names);
    Failure { gate: format!("concerns: {names}"), output: format!("contract concerns {names} were green at {} and red at {}; {}", landed.base, landed.head, outcome.unwrap_or_else(|why| format!("{commit} was not reverted: {why}"))) }
}

fn queue(landed: &Landed, lander: &Lander, commit: &str, names: &str) -> Result<String, String> {
    let message = git(landed.main, &["log", "-1", "--format=%B", commit])?;
    if sheriff::is_revert(&message) {
        return Err("it is itself a revert".to_owned());
    }
    let marker = lander.queue.join("concern-reverts").join(commit);
    if marker.exists() {
        return Ok(format!("the revert of {commit} is already queued"));
    }
    let worktree = candidate::lander_worktree(landed.main, "lander-concerns")?;
    candidate::ensure(landed.main, &worktree)?;
    if !sheriff::without(&worktree, landed.head, commit)? {
        return Err(format!("it no longer reverts cleanly onto {}", landed.head));
    }
    let branch = format!("concerns-revert-{}", &commit[..commit.len().min(12)]);
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = message.lines().next().unwrap_or_default();
    let text = format!("Revert {subject}\n\nContract concerns {names} were green before this landing and red at {}, so the after-landing run reverts it through the landing queue.\n\nReverts: {commit}\nConcern-red: {names}", landed.head);
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &text])?;
    sheriff::queue_revert(lander, &worktree, &branch)?;
    let _ = std::fs::create_dir_all(marker.parent().unwrap_or(&lander.queue)).and_then(|()| std::fs::write(&marker, &branch));
    Ok(format!("{commit} is reverted as {branch} through the landing queue"))
}

#[cfg(test)]
#[path = "concerns_tests.rs"]
mod tests;
