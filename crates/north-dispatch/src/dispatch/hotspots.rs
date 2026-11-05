//! The files a ready box's body names on its `Files:` line, which one dispatch run staffs to a
//! single lane at a time so two workers never edit the same file.
use super::facets::declared;
use super::issues::{Issue, IssueRef};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Claims {
    held: BTreeMap<String, IssueRef>,
}

impl Claims {
    /// The first file `issue` names that an earlier launch of this run already holds, with that launch's box.
    pub fn conflict(&self, issue: &Issue) -> Option<(String, &IssueRef)> {
        files(issue).into_iter().find_map(|file| self.held.get(&file).map(|holder| (file, holder)))
    }

    pub fn take(&mut self, issue: &Issue) {
        for file in files(issue) {
            self.held.entry(file).or_insert_with(|| issue.reference());
        }
    }
}

pub fn hold_reason(file: &str, holder: &IssueRef) -> String {
    format!("held for {holder}: both name {file}, and two lanes never edit one file at once")
}

fn files(issue: &Issue) -> Vec<String> {
    declared(issue, "Files")
        .into_iter()
        .flat_map(|line| line.split([',', ' ']))
        .map(|file| file.trim().trim_matches('`'))
        .filter(|file| file.contains('/') || file.contains('.'))
        .map(str::to_owned)
        .collect()
}
