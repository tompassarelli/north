use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::proc::{git, run};

const BATCH: usize = 25;

pub struct Landed {
    pub item: u64,
    pub commit: String,
    pub base: String,
    pub base_time: i64,
}

pub struct Box {
    pub item: u64,
    pub commit: String,
    pub base: String,
    pub text: String,
    pub changed: Vec<String>,
}

fn items_named(message: &str) -> Vec<u64> {
    let mut found = Vec::new();
    for (at, _) in message.match_indices("north#") {
        let digits: String = message[at + 6..].chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse() {
            found.push(number);
        }
    }
    found
}

fn subject_item(subject: &str) -> Option<u64> {
    let inside = subject.trim_end().strip_suffix(')')?.rsplit_once("(north#")?.1;
    inside.parse().ok()
}

pub fn landed(repo: &Path) -> Result<Vec<Landed>, String> {
    let log = git(repo, &["log", "origin/main", "--first-parent", "--format=%H%x1f%P%x1f%s%x1f%b%x1e"], None)?.text();
    let mut named: BTreeMap<u64, usize> = BTreeMap::new();
    let mut commits = Vec::new();
    for record in log.split('\x1e').map(str::trim).filter(|record| !record.is_empty()) {
        let fields: Vec<&str> = record.splitn(4, '\x1f').collect();
        let [commit, parents, subject, body] = fields[..] else { continue };
        let mut seen = items_named(subject);
        seen.extend(items_named(body));
        seen.sort_unstable();
        seen.dedup();
        seen.iter().for_each(|item| *named.entry(*item).or_default() += 1);
        if let (Some(item), Some(base)) = (subject_item(subject), parents.split_whitespace().next()) {
            commits.push((item, commit.to_owned(), base.to_owned()));
        }
    }
    let mut out = Vec::new();
    for (item, commit, base) in commits.into_iter().filter(|(item, ..)| named.get(item) == Some(&1)) {
        let base_time = git(repo, &["show", "-s", "--format=%ct", &base], None)?.text().trim().parse().map_err(|e| format!("{base}: {e}"))?;
        out.push(Landed { item, commit, base, base_time });
    }
    Ok(out)
}

fn issues(numbers: &[u64]) -> Result<BTreeMap<u64, Value>, String> {
    let fields = "number state title body createdAt userContentEdits(first: 100) { nodes { editedAt diff } }";
    let asked: String = numbers.iter().map(|n| format!("i{n}: issue(number: {n}) {{ {fields} }} ")).collect();
    let query = format!("{{ repository(owner: \"tompassarelli\", name: \"north\") {{ {asked} }} }}");
    let ran = run("gh", &["api", "graphql", "-f", &format!("query={query}")], &[], &[], None);
    let reply: Value = serde_json::from_slice(&ran.out).map_err(|e| format!("gh api graphql: {e}: {}", ran.err.trim()))?;
    let repository = &reply["data"]["repository"];
    Ok(numbers.iter().filter_map(|n| repository.get(format!("i{n}")).filter(|issue| !issue.is_null()).map(|issue| (*n, issue.clone()))).collect())
}

fn seconds(stamp: &str) -> i64 {
    let ran = run("date", &["-d", stamp, "+%s"], &[], &[], None);
    ran.text().trim().parse().unwrap_or(i64::MAX)
}

pub fn body_as_of(issue: &Value, at: i64) -> String {
    let edits: Vec<(i64, &str)> = issue["userContentEdits"]["nodes"].as_array().into_iter().flatten().filter_map(|edit| Some((seconds(edit["editedAt"].as_str()?), edit["diff"].as_str()?))).collect();
    if edits.is_empty() {
        return issue["body"].as_str().unwrap_or_default().to_owned();
    }
    let before = edits.iter().filter(|(when, _)| *when <= at).max_by_key(|(when, _)| *when);
    let oldest = edits.iter().min_by_key(|(when, _)| *when);
    before.or(oldest).map(|(_, body)| (*body).to_owned()).unwrap_or_default()
}

pub fn build(repo: &Path, wanted: usize, cache: &Path) -> Result<Vec<Box>, String> {
    if let Ok(held) = std::fs::read_to_string(cache) {
        let json: Value = serde_json::from_str(&held).map_err(|e| e.to_string())?;
        return Ok(json.as_array().into_iter().flatten().map(|b| Box {
            item: b["item"].as_u64().unwrap_or(0),
            commit: b["commit"].as_str().unwrap_or_default().to_owned(),
            base: b["base"].as_str().unwrap_or_default().to_owned(),
            text: b["text"].as_str().unwrap_or_default().to_owned(),
            changed: b["changed"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_owned)).collect(),
        }).collect());
    }
    let mut chosen = Vec::new();
    for group in landed(repo)?.chunks(BATCH) {
        let numbers: Vec<u64> = group.iter().map(|landed| landed.item).collect();
        let found = issues(&numbers)?;
        for landed in group {
            let Some(issue) = found.get(&landed.item).filter(|issue| issue["state"] == "CLOSED") else { continue };
            let changed: Vec<String> = git(repo, &["diff", "--name-only", &landed.base, &landed.commit], None)?.text().lines().map(str::to_owned).collect();
            if changed.is_empty() {
                continue;
            }
            let text = format!("{}\n\n{}", issue["title"].as_str().unwrap_or_default(), body_as_of(issue, landed.base_time));
            chosen.push(Box { item: landed.item, commit: landed.commit.clone(), base: landed.base.clone(), text, changed });
        }
        if chosen.len() >= wanted {
            break;
        }
    }
    chosen.truncate(wanted);
    let json: Vec<Value> = chosen.iter().map(|b| json!({"item": b.item, "commit": b.commit, "base": b.base, "text": b.text, "changed": b.changed})).collect();
    std::fs::write(cache, Value::Array(json).to_string()).map_err(|e| format!("{}: {e}", cache.display()))?;
    Ok(chosen)
}
