use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::github;

const NORTH: &str = "tompassarelli/north";

pub const CONFIRMING_TICKS: u32 = 2;

pub const STAFFED_ALARM: &str = "an alarm's body declares a `Category:` line, so dispatch would staff a worker on an issue that closes itself when its signal clears";

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
    fn body(&self, number: u64) -> Result<String, String>;
    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String>;
    fn create(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn edit(&mut self, number: u64, body: &str) -> Result<(), String>;
    fn close(&mut self, number: u64, comment: &str) -> Result<(), String>;
}

pub fn alarm(tracker: &mut dyn Tracker, breaches: &mut Breaches, title: &str, body: Option<&str>, closing: &str) -> Result<(), String> {
    if body.is_some_and(declares_category) {
        return Err(STAFFED_ALARM.to_owned());
    }
    if breaches.record(title, body.is_some()) || body.is_none() { settle(tracker, title, body, closing) } else { Ok(()) }
}

pub fn declares_category(body: &str) -> bool {
    body.lines().any(|line| line.trim().trim_start_matches(['-', '*', ' ']).split_once(':').is_some_and(|(name, _)| name.trim().trim_matches('*').eq_ignore_ascii_case("category")))
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

pub fn section_box(body: &str, heading: &str, key: &str, line: &str, add: bool) -> String {
    let mut lines: Vec<&str> = body.lines().collect();
    let Some(start) = lines.iter().position(|text| text.trim_end() == heading) else {
        if !add {
            return body.to_owned();
        }
        let kept = body.trim_end();
        return if kept.is_empty() { format!("{heading}\n{line}\n") } else { format!("{kept}\n\n{heading}\n{line}\n") };
    };
    let end = lines[start + 1..].iter().position(|text| text.starts_with("# ") || text.starts_with("## ")).map_or(lines.len(), |offset| start + 1 + offset);
    let boxed = |text: &str| ["- [ ] ", "- [x] ", "- [X] "].iter().find_map(|marker| text.trim_start().strip_prefix(marker)).is_some_and(|rest| rest.starts_with(key));
    match lines[start + 1..end].iter().position(|text| boxed(text)) {
        Some(offset) => lines[start + 1 + offset] = line,
        None if add => {
            let last = lines[start + 1..end].iter().rposition(|text| !text.trim().is_empty()).map_or(start, |offset| start + 1 + offset);
            lines.insert(last + 1, line);
        }
        None => return body.to_owned(),
    }
    let mut edited = lines.join("\n");
    edited.push('\n');
    edited
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

    fn body(&self, number: u64) -> Result<String, String> {
        let found = github::json(&["issue", "view", &number.to_string(), "-R", &self.repo, "--json", "body"])?;
        found["body"].as_str().map(str::to_owned).ok_or(format!("gh issue view {number}: no body"))
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

#[cfg(test)]
#[path = "rolling_issue_tests.rs"]
mod tests;
