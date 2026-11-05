use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[([ xX])\]\s+(.+)$").expect("box pattern"));
static NUMBERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\d+\.\s+(.+)$").expect("numbered pattern"));
static DONE_WHEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^#*\s*Done when:?\s*$").expect("done-when pattern"));
static DEPENDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:depends on|after)\s+(?:box(?:es)?\s+(\d+)|(?:([\w.-]+))?#(\d+))").expect("depends pattern"));

#[derive(Clone, Debug)]
pub struct GoalBox {
    pub key: String,
    pub item: i64,
    pub text: String,
    pub facets: String,
    pub deps: BTreeSet<String>,
    pub eta: Option<f64>,
    pub source: String,
}

fn marked(body: &str) -> Vec<(bool, String)> {
    let found = DONE_WHEN.find(body);
    let section = found.map_or(body, |at| &body[at.end()..]);
    let listed: Vec<(bool, String)> = BOX.captures_iter(section).map(|cap| (&cap[1] == " ", cap[2].to_owned())).collect();
    if !listed.is_empty() || found.is_none() {
        return listed;
    }
    let first = if section.trim().is_empty() { "" } else { section.split("\n\n").next().unwrap_or("") };
    NUMBERED.captures_iter(first).map(|cap| (true, cap[1].to_owned())).collect()
}

fn blockers(issue: &Value) -> BTreeSet<i64> {
    issue["blockedBy"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|node| node["state"].as_str().unwrap_or("OPEN").eq_ignore_ascii_case("OPEN"))
        .filter_map(|node| node["number"].as_i64())
        .collect()
}

/// Open Done-when boxes of the goal's issues, in issue order; deps come from `depends on`/`after` lines in a box (box N of the same
/// issue, or [repo]#N for every open box of that issue) and from GitHub blockedBy.
pub fn parse_boxes(project: &str, issues: &[Value]) -> Vec<GoalBox> {
    let numbers: BTreeSet<i64> = issues.iter().filter_map(|issue| issue["number"].as_i64()).collect();
    let mut boxes = Vec::new();
    let mut blocked = Vec::new();
    for issue in issues {
        let (number, body) = (issue["number"].as_i64().unwrap_or_default(), issue["body"].as_str().unwrap_or(""));
        let labels = issue["labels"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str());
        let facets_tail: Vec<String> = std::iter::once(body.to_owned()).chain(labels.map(|label| format!("Label: {label}"))).collect();
        for (index, (open, text)) in marked(body).into_iter().enumerate() {
            if !open {
                continue;
            }
            let facets = [vec![text.clone()], facets_tail.clone()].concat().join("\n");
            boxes.push(GoalBox { key: format!("{project}#{number}/{}", index + 1), item: number, text, facets, deps: BTreeSet::new(), eta: None, source: String::new() });
            blocked.push(blockers(issue));
        }
    }
    let keys: Vec<(String, i64)> = boxes.iter().map(|found| (found.key.clone(), found.item)).collect();
    let of_issue = |number: i64| keys.iter().filter(move |(_, item)| *item == number).map(|(key, _)| key.clone());
    for (at, found) in boxes.iter_mut().enumerate() {
        let mut deps = BTreeSet::new();
        for cap in DEPENDS.captures_iter(&found.text) {
            if let Some(local) = cap.get(1) {
                deps.insert(format!("{project}#{}/{}", found.item, local.as_str()));
            } else if cap.get(2).map_or(project, |repo| repo.as_str()) == project
                && let Ok(number) = cap[3].parse::<i64>()
                && numbers.contains(&number)
            {
                deps.extend(of_issue(number));
            }
        }
        for number in &blocked[at] {
            deps.extend(of_issue(*number));
        }
        deps.retain(|dep| *dep != found.key && keys.iter().any(|(key, _)| key == dep));
        found.deps = deps;
    }
    boxes
}
