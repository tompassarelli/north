use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::landing::red_check::{self, Failure};
use super::candidate::git;

const SCOPE: &str = "safe-push: the Rust checks cover ";
const CHECK: &str = "safe-push: land check";

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Episode {
    pub head: String,
    pub scope: BTreeSet<String>,
    pub failing: BTreeSet<Failure>,
}

impl Episode {
    pub fn opened(head: &str, log: &str, failing: BTreeSet<Failure>) -> Self {
        Self { head: head.to_owned(), scope: scope(log), failing }
    }

    pub fn read(marker: &Path) -> Option<Self> {
        let text = fs::read_to_string(marker).ok()?;
        let mut episode = Self::default();
        for line in text.lines() {
            match line.split_once('\t') {
                Some(("head", head)) => head.clone_into(&mut episode.head),
                Some(("scope", package)) => {
                    episode.scope.insert(package.to_owned());
                }
                Some(("failure", key)) => episode.failing.extend(Failure::from_key(key)),
                _ => {}
            }
        }
        Some(episode)
    }

    pub fn write(&self, marker: &Path) -> std::io::Result<()> {
        let mut text = format!("head\t{}\n", self.head);
        self.scope.iter().for_each(|package| text.push_str(&format!("scope\t{package}\n")));
        self.failing.iter().for_each(|failure| text.push_str(&format!("failure\t{}\n", failure.key())));
        fs::write(marker, text)
    }

    pub fn ended_by(&self, green_log: &str, repo: &Path, head: &str) -> bool {
        self.covered_by(green_log) || reverted_since(repo, &self.head, head)
    }

    fn covered_by(&self, green_log: &str) -> bool {
        let named: Vec<&Failure> = self.failing.iter().filter(|failure| matches!(failure, Failure::Cargo(_) | Failure::Harness(_))).collect();
        if !named.is_empty() {
            let passed = passed(green_log);
            return named.iter().all(|failure| passed.contains(*failure));
        }
        let ran = |command: &str| green_log.lines().map(red_check::log_text).any(|line| line.starts_with(CHECK) && line.contains(&format!(": {command} (")));
        let steps_ran = self.failing.iter().all(|failure| !matches!(failure, Failure::Step(command) if !ran(command)));
        steps_ran && self.scope.is_subset(&scope(green_log))
    }
}

fn scope(log: &str) -> BTreeSet<String> {
    let covered = log.lines().map(red_check::log_text).find_map(|line| line.strip_prefix(SCOPE)).unwrap_or_default();
    covered.split_whitespace().filter(|word| *word != "-p").map(str::to_owned).collect()
}

fn passed(log: &str) -> BTreeSet<Failure> {
    let mut found = BTreeSet::new();
    for line in log.lines().map(red_check::log_text) {
        if let Some(rest) = line.strip_prefix("PASS ") {
            found.extend(rest.split_whitespace().last().map(|name| Failure::Cargo(name.to_owned())));
        } else if let Some(rest) = line.strip_prefix("test ") {
            let name = rest.split_once(" ... ok").map(|(name, _)| name.trim());
            found.extend(name.map(|name| Failure::Cargo(name.to_owned())));
        } else if let Some(path) = line.strip_prefix("land check: ").and_then(|rest| rest.strip_suffix(" passed")) {
            found.insert(Failure::Harness(path.to_owned()));
        }
    }
    found
}

fn reverted_since(repo: &Path, red: &str, head: &str) -> bool {
    let Ok(messages) = git(repo, &["log", "--format=%B", &format!("{red}..{head}")]) else { return false };
    messages.lines().filter_map(|line| line.strip_prefix("Sheriff-Reverts:")).map(str::trim).any(|reverted| git(repo, &["merge-base", "--is-ancestor", reverted, red]).is_ok())
}
