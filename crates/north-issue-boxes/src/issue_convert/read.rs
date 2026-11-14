use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::foundation::clock::rfc3339_seconds;
use crate::platform::github;

const MOST_PAGES: usize = 100;

pub struct Issue {
    pub number: u64,
    pub open: bool,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub created: i64,
    pub children: u64,
}

#[derive(Default)]
pub struct Repository {
    pub private: bool,
    /// None when the repository has more labels than one page returns.
    pub labels: Option<BTreeSet<String>>,
    pub issues: Vec<Issue>,
}

fn query(closed_too: bool) -> String {
    let states = if closed_too { "[OPEN, CLOSED]" } else { "[OPEN]" };
    format!(
        "query($owner: String!, $name: String!, $cursor: String) {{ repository(owner: $owner, name: $name) {{ isPrivate labels(first: 100) {{ totalCount nodes {{ name }} }} issues(states: {states}, first: 100, after: $cursor) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ number closed title body createdAt labels(first: 30) {{ nodes {{ name }} }} subIssuesSummary {{ total }} }} }} }} }}"
    )
}

fn names(nodes: &Value) -> impl Iterator<Item = String> + '_ {
    nodes.as_array().into_iter().flatten().filter_map(|node| node["name"].as_str().map(str::to_owned))
}

fn issue(node: &Value) -> Option<Issue> {
    Some(Issue {
        number: node["number"].as_u64()?,
        open: node["closed"].as_bool() == Some(false),
        title: node["title"].as_str().unwrap_or_default().to_owned(),
        body: node["body"].as_str().unwrap_or_default().to_owned(),
        labels: names(&node["labels"]["nodes"]).collect(),
        created: node["createdAt"].as_str().and_then(rfc3339_seconds).unwrap_or(0.0) as i64,
        children: node["subIssuesSummary"]["total"].as_u64().unwrap_or(0),
    })
}

pub struct Target {
    pub children: u64,
    pub labels: BTreeMap<String, String>,
    pub issue_types: BTreeMap<String, String>,
}

const TARGET_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { labels(first: 100) { totalCount nodes { id name } } issueTypes(first: 20) { nodes { id name } } issue(number: $number) { subIssuesSummary { total } } } }";

fn pairs(nodes: &Value) -> BTreeMap<String, String> {
    nodes.as_array().into_iter().flatten().filter_map(|node| Some((node["name"].as_str()?.to_owned(), node["id"].as_str()?.to_owned()))).collect()
}

/// The type and label ids a child is created with and the parent's children count: one read of the repository and its parent.
pub fn target(owner: &str, name: &str, number: u64) -> Result<Target, String> {
    let reference = format!("{owner}/{name}#{number}");
    let (query, owner_field, name_field, number_field) = (format!("query={TARGET_QUERY}"), format!("owner={owner}"), format!("name={name}"), format!("number={number}"));
    let args = ["api", "graphql", "-f", query.as_str(), "-f", owner_field.as_str(), "-f", name_field.as_str(), "-F", number_field.as_str()];
    let answer = github::json(&args)?;
    if answer.get("errors").is_some() {
        return Err(format!("GitHub's answer for {reference} holds errors: {}", answer["errors"]));
    }
    let found = &answer["data"]["repository"];
    let issue = &found["issue"];
    if !issue.is_object() {
        return Err(format!("{reference} is not an issue"));
    }
    let labels = pairs(&found["labels"]["nodes"]);
    if found["labels"]["totalCount"].as_u64() != Some(labels.len() as u64) {
        return Err(format!("{owner}/{name} has more labels than one page returns"));
    }
    Ok(Target {
        children: issue["subIssuesSummary"]["total"].as_u64().unwrap_or(0),
        labels,
        issue_types: pairs(&found["issueTypes"]["nodes"]),
    })
}

/// A repository's visibility, labels and issues with their bodies: the one GitHub read a run makes, a page per 100
/// issues, each an identical call the GitHub broker answers from its cache while it is fresh.
pub fn repository(owner: &str, name: &str, closed_too: bool) -> Result<Repository, String> {
    let (query, owner_field, name_field) = (format!("query={}", query(closed_too)), format!("owner={owner}"), format!("name={name}"));
    let (mut read, mut cursor) = (Repository::default(), None::<String>);
    for _ in 0..MOST_PAGES {
        let after = cursor.as_ref().map(|cursor| format!("cursor={cursor}"));
        let mut args = vec!["api", "graphql", "-f", query.as_str(), "-f", owner_field.as_str(), "-f", name_field.as_str()];
        if let Some(after) = &after {
            args.extend(["-f", after.as_str()]);
        }
        let answer = github::json(&args)?;
        let found = &answer["data"]["repository"];
        let page = &found["issues"];
        let nodes = page["nodes"].as_array().filter(|_| answer.get("errors").is_none()).ok_or_else(|| format!("GitHub's answer for {owner}/{name} holds no issues: {}", answer["errors"]))?;
        read.private = found["isPrivate"] == true;
        let labels: BTreeSet<String> = names(&found["labels"]["nodes"]).collect();
        read.labels = (found["labels"]["totalCount"].as_u64() == Some(labels.len() as u64)).then_some(labels);
        read.issues.extend(nodes.iter().filter_map(issue));
        if page["pageInfo"]["hasNextPage"] != true {
            return Ok(read);
        }
        cursor = page["pageInfo"]["endCursor"].as_str().map(str::to_owned);
    }
    Err(format!("{owner}/{name} has more than {} issues, more than one run reads", MOST_PAGES * 100))
}
