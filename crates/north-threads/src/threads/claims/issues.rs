//! Whether each claimed GitHub issue exists, is open, and when it was opened, in one GraphQL call.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::Issue;
use crate::foundation::clock::Timestamp;
use crate::threads::catalog::CATALOG;

/// The fields a standing reads of each issue.
pub const STANDING: &str = "state createdAt";

/// The fields a box claim reads of its one issue: its standing and the body that holds its boxes.
const WITH_BODY: &str = "state createdAt body";

/// What a refusal of a box its issue lacks says, which `north fleet launch` tells from a ledger it could not read.
pub const NO_BOX: &str = "names no box of";

pub trait Issues {
    /// Each `(repo, number)` asked for, as GitHub shows it.
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue>;
    /// `repo`#`number` as GitHub shows it, with its body, `None` when the body could not be read, in the one call [`Issues::read`] makes.
    fn with_body(&self, repo: &str, number: u64) -> (Issue, Option<String>);
}

pub struct GitHub;

impl GitHub {
    fn ask(items: &BTreeSet<(String, u64)>, fields: &str) -> Value {
        let query = format!("query={}", query(items, fields));
        let output = crate::platform::github::run_owned(&["api".into(), "graphql".into(), "-f".into(), query]);
        output.ok().and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok()).unwrap_or(Value::Null)
    }
}

impl Issues for GitHub {
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue> {
        if items.is_empty() {
            return BTreeMap::new();
        }
        answers(items, &Self::ask(items, STANDING))
    }

    fn with_body(&self, repo: &str, number: u64) -> (Issue, Option<String>) {
        let items = BTreeSet::from([(repo.to_owned(), number)]);
        let reply = Self::ask(&items, WITH_BODY);
        let issue = answers(&items, &reply).remove(&(repo.to_owned(), number)).unwrap_or(Issue::Unread);
        (issue, body(&reply, number))
    }
}

/// The body of the one issue `number` a reply to a `WITH_BODY` query names.
fn body(reply: &Value, number: u64) -> Option<String> {
    reply["data"]["r0"][format!("i{number}")]["body"].as_str().map(str::to_owned)
}

/// Refuses a box slug the issue's `body` lacks, ticked or not: a claim on a box its issue lacks holds no box in dispatch, so the refusal names the boxes the issue has. A native item or an unread body refuses nothing.
pub fn box_of_issue(body: Option<&str>, (repo, number): (&str, u64), slug: &str) -> Result<(), String> {
    match body.filter(|_| !CATALOG.is_native(repo)) {
        Some(body) if !super::names_box(body, slug) => {
            let shown: Vec<String> = super::boxes(body).map(|text| text.chars().take(90).collect()).collect();
            Err(format!("--box {slug} {NO_BOX} {repo}#{number}; give the first words of one of its boxes: {}", shown.join(" | ")))
        }
        _ => Ok(()),
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
pub fn query(items: &BTreeSet<(String, u64)>, fields: &str) -> String {
    let lookups: Vec<String> = repositories(items)
        .into_iter()
        .enumerate()
        .map(|(index, (repo, numbers))| {
            let issues: Vec<String> = numbers.iter().map(|number| format!("i{number}: issue(number: {number}) {{ {fields} }}")).collect();
            format!("r{index}: repository(owner: \"{}\", name: \"{repo}\") {{ {} }}", CATALOG.owner, issues.join(" "))
        })
        .collect();
    format!("query {{ {} }}", lookups.join(" "))
}

/// GitHub's reply read item by item; a null lookup is an issue that does not exist, since GitHub
/// answers the rest of the query beside a NOT_FOUND error.
pub fn answers(items: &BTreeSet<(String, u64)>, reply: &Value) -> BTreeMap<(String, u64), Issue> {
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
