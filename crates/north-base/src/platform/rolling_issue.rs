use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::github;

const NORTH: &str = "tompassarelli/north";

pub const CONFIRMING_TICKS: u32 = 2;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Breaches(BTreeMap<String, u32>);

impl Breaches {
    pub fn record(&mut self, key: &str, breached: bool) -> bool {
        if !breached {
            self.0.remove(key);
            return false;
        }
        let streak = self.0.entry(key.to_owned()).or_default();
        *streak = streak.saturating_add(1);
        *streak >= CONFIRMING_TICKS
    }

    pub fn tick(&mut self, breaching: &BTreeSet<String>) -> BTreeSet<String> {
        self.0.retain(|key, _| breaching.contains(key));
        breaching.iter().filter(|key| self.record(key, true)).cloned().collect()
    }
}
pub struct Issue {
    pub number: u64,
    pub body: String,
}

pub trait Tracker {
    fn open(&self, title: &str) -> Result<Option<Issue>, String>;
    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String>;
    fn create(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn edit(&mut self, number: u64, body: &str) -> Result<(), String>;
    fn close(&mut self, number: u64, comment: &str) -> Result<(), String>;
}

pub fn alarm(tracker: &mut dyn Tracker, breaches: &mut Breaches, title: &str, body: Option<&str>, closing: &str) -> Result<(), String> {
    if breaches.record(title, body.is_some()) || body.is_none() { settle(tracker, title, body, closing) } else { Ok(()) }
}

pub fn settle(tracker: &mut dyn Tracker, title: &str, body: Option<&str>, closing: &str) -> Result<(), String> {
    match (tracker.open(title)?, body) {
        (None, None) => {}
        (Some(issue), None) => tracker.close(issue.number, closing)?,
        (None, Some(body)) => tracker.create(title, body)?,
        (Some(issue), Some(body)) => {
            if issue.body.trim() != body.trim() {
                tracker.edit(issue.number, body)?;
            }
        }
    }
    Ok(())
}

pub struct Gh {
    repo: String,
    preferred_labels: Vec<String>,
}

impl Gh {
    pub fn north() -> Self {
        Self::of(NORTH, &[])
    }

    pub fn of(repo: &str, preferred_labels: &[&str]) -> Self {
        Self { repo: repo.to_owned(), preferred_labels: preferred_labels.iter().map(|label| (*label).to_owned()).collect() }
    }

    fn present_labels(&self) -> Result<Vec<String>, String> {
        if self.preferred_labels.is_empty() {
            return Ok(Vec::new());
        }
        let listed = github::json(&["label", "list", "-R", &self.repo, "--json", "name", "-L", "500"])?;
        let present: Vec<&str> = listed.as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()).collect();
        Ok(self.preferred_labels.iter().filter(|label| present.contains(&label.as_str())).cloned().collect())
    }
}

impl Tracker for Gh {
    fn open(&self, title: &str) -> Result<Option<Issue>, String> {
        let search = format!("in:title \"{title}\"");
        let found = github::json(&["issue", "list", "-R", &self.repo, "--state", "open", "--search", &search, "--json", "number,title,body"])?;
        Ok(found.as_array().into_iter().flatten().find(|issue| issue["title"] == title).and_then(|issue| {
            Some(Issue { number: issue["number"].as_u64()?, body: issue["body"].as_str().unwrap_or_default().to_owned() })
        }))
    }

    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String> {
        let found = github::json(&["issue", "list", "-R", &self.repo, "--state", "open", "--search", search, "--json", "number,title"])?;
        Ok(found.as_array().into_iter().flatten().filter_map(|issue| Some((issue["number"].as_u64()?, issue["title"].as_str()?.to_owned()))).collect())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        let labels = self.present_labels()?;
        let mut command = vec!["issue", "create", "-R", &self.repo, "--title", title, "--body", body];
        for label in &labels {
            command.extend(["--label", label]);
        }
        succeeded(github::run(&command), "issue create")
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "edit", &number.to_string(), "-R", &self.repo, "--body", body]), "issue edit")
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "close", &number.to_string(), "-R", &self.repo, "--comment", comment]), "issue close")
    }
}

fn succeeded(out: std::io::Result<std::process::Output>, what: &str) -> Result<(), String> {
    let out = out.map_err(|error| format!("gh {what}: {error}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("gh {what}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
