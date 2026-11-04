use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Case {
    pub test: String,
    pub name: String,
    pub passed: bool,
}

impl Case {
    pub fn row(&self) -> String {
        serde_json::json!({"test": self.test, "case": self.name, "outcome": if self.passed { "pass" } else { "fail" }}).to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Env {
    pub south_revision: String,
    pub nixos_config_revision: String,
    pub shell_closure: String,
    pub runner: String,
}

impl Env {
    pub fn hash(&self) -> String {
        digest(&[&self.south_revision, &self.nixos_config_revision, &self.shell_closure, &self.runner])
    }
}

fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Key {
    pub tree: String,
    pub command: String,
    pub env: String,
}

impl Key {
    pub fn new(tree: &str, command: &str, env: &Env) -> Self {
        Key { tree: tree.to_owned(), command: command.to_owned(), env: env.hash() }
    }

    fn file(&self, root: &std::path::Path) -> io::Result<PathBuf> {
        if self.tree.is_empty() || !self.tree.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a results key names a tree by its hex object id"));
        }
        Ok(root.join(&self.tree).join(format!("{}.tsv", &digest(&[&self.command, &self.env])[..32])))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Seen {
    pub passed: bool,
    pub failed: bool,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Results {
    cases: BTreeMap<(String, String), Seen>,
}

impl FromIterator<Case> for Results {
    fn from_iter<I: IntoIterator<Item = Case>>(cases: I) -> Self {
        let mut results = Results::default();
        for case in cases {
            results.see(&case.test, &case.name, case.passed);
        }
        results
    }
}

impl Results {
    pub fn see(&mut self, test: &str, case: &str, passed: bool) {
        let seen = self.cases.entry((test.to_owned(), case.to_owned())).or_default();
        if passed {
            seen.passed = true;
        } else {
            seen.failed = true;
        }
    }

    pub fn extend(&mut self, other: &Results) {
        for (id, seen) in &other.cases {
            let mine = self.cases.entry(id.clone()).or_default();
            mine.passed |= seen.passed;
            mine.failed |= seen.failed;
        }
    }

    pub fn get(&self, test: &str, case: &str) -> Option<Seen> {
        self.cases.get(&(test.to_owned(), case.to_owned())).copied()
    }

    pub fn failed(&self) -> Vec<(&str, &str)> {
        self.select(|seen| seen.failed)
    }

    pub fn flaky(&self) -> Vec<(&str, &str)> {
        self.select(|seen| seen.failed && seen.passed)
    }

    fn select(&self, keep: impl Fn(&Seen) -> bool) -> Vec<(&str, &str)> {
        self.cases.iter().filter(|(_, seen)| keep(seen)).map(|((test, case), _)| (test.as_str(), case.as_str())).collect()
    }
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: PathBuf) -> Self {
        Store { root }
    }

    pub fn default_root() -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        });
        state.join("safe-push/case-results")
    }

    pub fn record(&self, key: &Key, cases: &[Case]) -> io::Result<()> {
        let path = key.file(&self.root)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body: String = cases
            .iter()
            .map(|case| format!("{}\t{}\t{}\n", if case.passed { "pass" } else { "fail" }, field(&case.test), field(&case.name)))
            .collect();
        OpenOptions::new().create(true).append(true).open(path)?.write_all(body.as_bytes())
    }

    pub fn lookup(&self, key: &Key) -> Option<Results> {
        let text = fs::read_to_string(key.file(&self.root).ok()?).ok()?;
        let mut results = Results::default();
        for line in text.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(verdict), Some(test), Some(case)) = (parts.next(), parts.next(), parts.next()) else { continue };
            match verdict {
                "pass" => results.see(test, case, true),
                "fail" => results.see(test, case, false),
                _ => {}
            }
        }
        Some(results)
    }
}

fn field(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

#[cfg(test)]
#[path = "results_tests.rs"]
mod tests;
