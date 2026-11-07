use super::results::{Results, Seen};
use std::collections::{BTreeMap, BTreeSet};
use time::{Duration, OffsetDateTime};

pub const QUARANTINE_TTL: Duration = Duration::hours(24);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CaseId {
    pub test: String,
    pub case: String,
}

impl CaseId {
    pub fn new(test: &str, case: &str) -> Self {
        CaseId { test: test.to_owned(), case: case.to_owned() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    BaseRed,
    Flaky,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub reason: Reason,
    pub issue: Option<String>,
    pub tree: String,
    pub opened_at: OffsetDateTime,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Quarantine {
    entries: BTreeMap<CaseId, Entry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Land,
    Refuse { new: Vec<CaseId> },
    Hold { files: Vec<String> },
}

pub fn green(seen: Seen) -> bool {
    seen.passed && !seen.failed
}

impl Quarantine {
    pub fn get(&self, test: &str, case: &str) -> Option<&Entry> {
        self.entries.get(&CaseId::new(test, case))
    }

    pub fn cases(&self) -> impl Iterator<Item = (&CaseId, &Entry)> {
        self.entries.iter()
    }

    pub fn observe_base(&mut self, tree: &str, base: &Results, now: OffsetDateTime) {
        self.entries.retain(|id, entry| {
            let passed_on_new_tree = entry.tree != tree && base.get(&id.test, &id.case).is_some_and(green);
            entry.reason == Reason::Flaky || !passed_on_new_tree
        });
        let flaky: BTreeSet<(&str, &str)> = base.flaky().into_iter().collect();
        for (test, case) in base.failed() {
            let reason = if flaky.contains(&(test, case)) { Reason::Flaky } else { Reason::BaseRed };
            let entry = self.entries.entry(CaseId::new(test, case)).or_insert_with(|| Entry { reason, issue: None, tree: tree.to_owned(), opened_at: now });
            if reason == Reason::Flaky {
                entry.reason = Reason::Flaky;
            }
        }
    }

    pub fn file_issue(&mut self, id: &CaseId, issue: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.issue = Some(issue.to_owned());
        }
    }

    pub fn landed_naming(&mut self, issue: &str) {
        self.entries.retain(|_, entry| !(entry.reason == Reason::Flaky && entry.issue.as_deref() == Some(issue)));
    }

    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter_map(|line| {
                let [reason, tree, opened_at, issue, test, case] = line.splitn(6, '\t').collect::<Vec<_>>()[..] else { return None };
                let reason = match reason {
                    "base-red" => Reason::BaseRed,
                    "flaky" => Reason::Flaky,
                    _ => return None,
                };
                let opened_at = OffsetDateTime::from_unix_timestamp(opened_at.parse().ok()?).ok()?;
                let issue = (issue != "-").then(|| issue.to_owned());
                Some((CaseId::new(test, case), Entry { reason, issue, tree: tree.to_owned(), opened_at }))
            })
            .collect();
        Quarantine { entries }
    }

    pub fn render(&self) -> String {
        self.entries
            .iter()
            .map(|(id, entry)| {
                let reason = match entry.reason {
                    Reason::BaseRed => "base-red",
                    Reason::Flaky => "flaky",
                };
                let field = |text: &str| text.replace(['\t', '\n', '\r'], " ");
                let issue = entry.issue.as_deref().map_or_else(|| "-".to_owned(), field);
                format!("{reason}\t{}\t{}\t{issue}\t{}\t{}\n", entry.tree, entry.opened_at.unix_timestamp(), field(&id.test), field(&id.case))
            })
            .collect()
    }

    pub fn stale_files(&self, selected: &[String], now: OffsetDateTime) -> Vec<String> {
        let selected: BTreeSet<&str> = selected.iter().map(String::as_str).collect();
        let stale: BTreeSet<&str> = self
            .entries
            .iter()
            .filter(|(id, entry)| now - entry.opened_at > QUARANTINE_TTL && selected.contains(id.test.as_str()))
            .map(|(id, _)| id.test.as_str())
            .collect();
        stale.into_iter().map(str::to_owned).collect()
    }
}

pub fn new_failures(base: &Results, candidate: &Results, quarantine: &Quarantine) -> Vec<CaseId> {
    let base_red: BTreeSet<(&str, &str)> = base.failed().into_iter().collect();
    candidate
        .failed()
        .into_iter()
        .filter(|&(test, case)| !base_red.contains(&(test, case)) && quarantine.get(test, case).is_none())
        .map(|(test, case)| CaseId::new(test, case))
        .collect()
}

pub fn judge(base: &Results, candidate: &Results, quarantine: &Quarantine, selected: &[String], now: OffsetDateTime) -> Verdict {
    let new = new_failures(base, candidate, quarantine);
    if !new.is_empty() {
        return Verdict::Refuse { new };
    }
    let files = quarantine.stale_files(selected, now);
    if files.is_empty() { Verdict::Land } else { Verdict::Hold { files } }
}

#[cfg(test)]
#[path = "judge_tests.rs"]
mod tests;
