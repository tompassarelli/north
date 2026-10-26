//! Whether each claimed GitHub issue exists, is open, and when it was opened, in one GraphQL call.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::Issue;
use crate::foundation::clock::Timestamp;
use crate::threads::catalog::CATALOG;

pub(crate) trait Issues {
    /// Each `(repo, number)` asked for, as GitHub shows it.
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue>;
}

pub(crate) struct GitHub;

impl Issues for GitHub {
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue> {
        if items.is_empty() {
            return BTreeMap::new();
        }
        let query = format!("query={}", query(items));
        let output = crate::platform::github::run_owned(&["api".into(), "graphql".into(), "-f".into(), query]);
        let reply = output.ok().and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok()).unwrap_or(Value::Null);
        answers(items, &reply)
    }
}

fn repositories(items: &BTreeSet<(String, u64)>) -> BTreeMap<&str, Vec<u64>> {
    let mut repos: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for (repo, number) in items {
        repos.entry(repo).or_default().push(*number);
    }
    repos
}

/// One aliased `issue` lookup per item, grouped under one aliased `repository` per repo.
pub(crate) fn query(items: &BTreeSet<(String, u64)>) -> String {
    let lookups: Vec<String> = repositories(items)
        .into_iter()
        .enumerate()
        .map(|(index, (repo, numbers))| {
            let issues: Vec<String> = numbers.iter().map(|number| format!("i{number}: issue(number: {number}) {{ state createdAt }}")).collect();
            format!("r{index}: repository(owner: \"{}\", name: \"{repo}\") {{ {} }}", CATALOG.owner, issues.join(" "))
        })
        .collect();
    format!("query {{ {} }}", lookups.join(" "))
}

/// GitHub's reply read item by item; a null lookup is an issue that does not exist, since GitHub
/// answers the rest of the query beside a NOT_FOUND error.
pub(crate) fn answers(items: &BTreeSet<(String, u64)>, reply: &Value) -> BTreeMap<(String, u64), Issue> {
    let data = &reply["data"];
    let mut found = BTreeMap::new();
    for (index, (repo, numbers)) in repositories(items).into_iter().enumerate() {
        let repository = data.get(format!("r{index}"));
        for number in numbers {
            let issue = match repository {
                None => Issue::Unread,
                Some(Value::Null) => Issue::Missing,
                Some(repository) => match repository.get(format!("i{number}")) {
                    None => Issue::Unread,
                    Some(Value::Null) => Issue::Missing,
                    Some(issue) if issue["state"] == "OPEN" => issue["createdAt"]
                        .as_str()
                        .and_then(|at| Timestamp::parse(at).ok())
                        .map_or(Issue::Unread, |opened| Issue::Open { opened: opened.unix_seconds() }),
                    Some(issue) if issue["state"].is_string() => Issue::Closed,
                    Some(_) => Issue::Unread,
                },
            };
            found.insert((repo.to_owned(), number), issue);
        }
    }
    found
}
