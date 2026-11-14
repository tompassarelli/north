use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Item:\s*(?:[A-Za-z0-9_.-]+/)?([A-Za-z0-9_.-]+)#(\d+)").expect("item pattern"));

/// The branches with commits not on origin/main, by the issue number of `name` their commits' `Item:` lines name.
pub struct Lanes(BTreeMap<u64, BTreeSet<String>>);

impl Lanes {
    pub fn read(checkout: &Path, name: &str) -> Result<Self, String> {
        if !checkout.is_dir() {
            return Ok(Self(BTreeMap::new()));
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(checkout)
            .args(["log", "--branches", "--not", "refs/remotes/origin/main", "--source", "--format=%x01%S%n%B"])
            .output()
            .map_err(|error| format!("git log in {}: {error}", checkout.display()))?;
        if !output.status.success() {
            return Err(format!("git log in {}: {}", checkout.display(), String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(Self::parse(&String::from_utf8_lossy(&output.stdout), name))
    }

    pub(super) fn parse(log: &str, name: &str) -> Self {
        let mut found: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
        for commit in log.split('\u{1}') {
            let (branch, body) = commit.split_once('\n').unwrap_or((commit, ""));
            for item in ITEM.captures_iter(body).filter(|item| item[1].eq_ignore_ascii_case(name)) {
                if let Ok(number) = item[2].parse() {
                    found.entry(number).or_default().insert(branch.trim_start_matches("refs/heads/").to_owned());
                }
            }
        }
        Self(found)
    }

    pub fn naming(&self, number: u64) -> Vec<String> {
        self.0.get(&number).into_iter().flatten().cloned().collect()
    }
}
