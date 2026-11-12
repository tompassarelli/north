use super::issues::Children;
use super::*;
use north_github_mirror::store::{self, Mirror, OPEN_ISSUES, key};

pub type OpenIssues = (Vec<Issue>, BTreeSet<IssueRef>);

fn touched(node: &Value) -> f64 {
    let comment = node["comments"]["nodes"].as_array().and_then(|nodes| nodes.last()).unwrap_or(&Value::Null);
    [&node["lastEditedAt"], &comment["createdAt"], &comment["updatedAt"]].into_iter().filter_map(|time| crate::foundation::clock::rfc3339_seconds(time.as_str()?)).fold(0.0, f64::max)
}

pub fn issue(repo: &str, node: &Value) -> Option<Issue> {
    let open_blockers = node["blockedBy"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|blocker| blocker["state"] == "OPEN")
        .filter_map(|blocker| Some(IssueRef::new(blocker["repository"]["name"].as_str()?, blocker["number"].as_u64()?)))
        .collect();
    let comments: Vec<Value> = node["comments"]["nodes"].as_array().cloned().unwrap_or_default();
    Some(Issue {
        repo: repo.into(),
        number: node["number"].as_u64()?,
        title: node["title"].as_str().unwrap_or_default().into(),
        body: node["body"].as_str().unwrap_or_default().into(),
        labels: node["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str().map(str::to_owned)).collect(),
        open_blockers,
        comments,
        touched: touched(node),
        created: node["createdAt"].as_str().and_then(crate::foundation::clock::rfc3339_seconds).unwrap_or(0.0),
        stop: Default::default(),
        needs: Vec::new(),
        landings: 0,
        parent: node["parent"]["number"].as_u64().zip(node["parent"]["repository"]["name"].as_str()).map(|(number, name)| IssueRef::new(name, number)),
        kind: node["issueType"]["name"].as_str().map(str::to_owned),
        assignees: node["assignees"]["nodes"].as_array().into_iter().flatten().filter_map(|who| who["login"].as_str().map(str::to_owned)).collect(),
        children: Children { total: node["subIssuesSummary"]["total"].as_u64().unwrap_or(0), completed: node["subIssuesSummary"]["completed"].as_u64().unwrap_or(0) },
    })
}

pub trait Upstream {
    fn open_issue_nodes(&self, repo: &str) -> Result<Vec<Value>, String>;
    fn still_open(&self, reference: &IssueRef) -> Option<bool>;
}

pub struct Live;

impl Upstream for Live {
    fn open_issue_nodes(&self, repo: &str) -> Result<Vec<Value>, String> {
        store::fetch_open_issues(repo)
    }

    fn still_open(&self, reference: &IssueRef) -> Option<bool> {
        let (number, repo) = (reference.number.to_string(), format!("{OWNER}/{}", reference.repo));
        let state = crate::platform::github::json(&["issue", "view", &number, "-R", &repo, "--json", "state"]).ok()?;
        Some(state["state"] == "OPEN")
    }
}

fn nodes(repo: &str, mirror: &Mirror, at: f64, upstream: &dyn Upstream, notes: &mut Vec<String>) -> Result<Vec<Value>, String> {
    if let Some(Value::Array(nodes)) = mirror.fresh(&key(repo, OPEN_ISSUES), at) {
        return Ok(nodes);
    }
    notes.push(format!("{repo} issues read from GitHub: {} holds no copy under {:.0} min old", mirror.path().display(), store::FRESH_S / 60.0));
    upstream.open_issue_nodes(repo)
}

fn mirrored_open(reference: &IssueRef, mirror: &Mirror, at: f64) -> Option<bool> {
    let nodes = mirror.fresh(&key(&reference.repo, OPEN_ISSUES), at)?;
    Some(nodes.as_array()?.iter().any(|node| node["number"].as_u64() == Some(reference.number)))
}

/// Why a box the mirror listed may not launch by GitHub's own copy of its issue (`gh issue view --json state,body`): the
/// issue closed or its last box was ticked after the mirror copied it.
pub fn gone(node: &Value, box_text: Option<&str>) -> Option<String> {
    if node["state"] != "OPEN" {
        return Some(format!("{} on GitHub, though the mirror listed it open", node["state"].as_str().unwrap_or("unread").to_lowercase()));
    }
    let body = node["body"].as_str().unwrap_or_default();
    if let Some(text) = box_text.filter(|text| issues::box_ticked(body, text)) {
        return Some(format!("its box {text:?} is ticked on GitHub"));
    }
    (issues::unticked_boxes(body) == 0).then(|| "no unticked box on GitHub, though the mirror listed one".to_owned())
}

/// `gone` on the issue as GitHub holds it now; an unreadable issue is no reason to hold the launch.
pub fn recheck(reference: &IssueRef, box_text: Option<&str>) -> Option<String> {
    let (number, repo) = (reference.number.to_string(), format!("{OWNER}/{}", reference.repo));
    gone(&crate::platform::github::json(&["issue", "view", &number, "-R", &repo, "--json", "state,body"]).ok()?, box_text)
}

pub fn open_issues(repos: &[String], at: f64, notes: &mut Vec<String>) -> OpenIssues {
    read_open_issues(repos, &Mirror::local(), at, &Live, notes)
}

/// The staffed repositories' open issues, and every open issue they or their plans wait on, read from the GitHub mirror while it holds a fresh copy.
pub fn read_open_issues(repos: &[String], mirror: &Mirror, at: f64, upstream: &dyn Upstream, notes: &mut Vec<String>) -> OpenIssues {
    let mut found = Vec::new();
    for repo in repos {
        match nodes(repo, mirror, at, upstream, notes) {
            Ok(nodes) => found.extend(nodes.iter().filter_map(|node| issue(repo, node))),
            Err(why) => notes.push(format!("{repo} issues unread: {why}")),
        }
    }
    let mut open: BTreeSet<IssueRef> = found.iter().map(Issue::reference).collect();
    let planned = issues::planned_predecessors(&found);
    let named = found.iter().flat_map(|issue| issues::predecessors(issue).unwrap_or_default()).chain(planned.into_values().flatten());
    let elsewhere: BTreeSet<IssueRef> = named.filter(|reference| !repos.contains(&reference.repo)).collect();
    for reference in elsewhere {
        if mirrored_open(&reference, mirror, at).or_else(|| upstream.still_open(&reference)) != Some(false) {
            open.insert(reference);
        }
    }
    (found, open)
}
