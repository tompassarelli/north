use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::foundation::clock::Timestamp;
use crate::platform::records_mirror;

use super::responsible::{Blame, responsible};
use super::{Conclusion, Landing, Run, ancestor, changed, git_lines, short};

const CLASS: &str = "selection-miss";

pub fn name_lanes(repo: &Path, runs: &[Run], failing: Vec<(&Run, Vec<String>)>, outcomes: &BTreeMap<String, BTreeSet<String>>, landings: &[Landing], ledger: &Path) {
    let landed = landed(ledger);
    let mut blamed = BTreeSet::new();
    let mut tellers = Tellers { north: north(), repo, nodes: None };
    for (run, tests) in failing {
        let Some(green) = runs.iter().rev().find(|earlier| earlier.created < run.created && earlier.conclusion == Conclusion::Success && ancestor(repo, &earlier.sha, &run.sha)) else {
            println!("{} at {}: no full run in the window passed before it, so no lane is named", run.source, short(&run.sha));
            continue;
        };
        let chain = first_parents(repo, &green.sha, &run.sha);
        for test in tests {
            match responsible(&chain, outcomes, &landed, &test) {
                Blame::Unjudged(why) => println!("{test} at {}: {why}", short(&run.sha)),
                Blame::Lane { lane, sha } => {
                    if blamed.insert((lane.clone(), test.clone())) {
                        tellers.judge(landings, &lane, &sha, &test, run);
                    }
                }
            }
        }
    }
}

struct Tellers<'a> {
    north: PathBuf,
    repo: &'a Path,
    nodes: Option<Vec<String>>,
}

impl Tellers<'_> {
    fn judge(&mut self, landings: &[Landing], lane: &str, sha: &str, test: &str, run: &Run) {
        let parent = rev_parse(self.repo, &format!("{sha}^")).unwrap_or_default();
        let Some(own) = landings.iter().find(|landing| landing.head == sha && landing.base == parent) else {
            println!("{test}: {lane} at {} turned it red, and no selection of its own landing is stored", short(sha));
            return;
        };
        if own.selected.holds(test) {
            println!("{test}: {lane} at {} turned it red though its selection held it", short(sha));
            return;
        }
        let files = changed(self.repo, &parent, sha);
        let key = format!("{CLASS}:{lane}:{test}");
        let detected = Timestamp::parse_rfc3339(&run.created).map_or_else(records_mirror::now, |at| at.unix_seconds_f64()).to_string();
        let detail = format!("{test}; {}", files.join(" "));
        let recorded = self.command(&["ledger", "incident", "record", "--key", &key, "--class", CLASS, "--noticed-by", "lander", "--repo", "north", "--subject", lane, "--detected-at", &detected, "--detail", &detail]);
        let node = self.node(test);
        let told = files.iter().filter(|file| self.command(&["graph", "tell", "--writer", CLASS, &node, ":guards", file])).count();
        println!("{CLASS} {lane} {test}: {} left it out of its selection; {}; told {told} of {} ({node} :guards FILE) edges", short(sha), if recorded { "incident recorded" } else { "the incident record failed" }, files.len());
    }

    fn command(&self, arguments: &[&str]) -> bool {
        Command::new(&self.north).args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).status().is_ok_and(|status| status.success())
    }

    fn node(&mut self, test: &str) -> String {
        let nodes = self.nodes.get_or_insert_with(|| test_nodes(&self.north));
        let suffix = format!("::{test}");
        let mut found = nodes.iter().filter(|node| node.ends_with(&suffix));
        match (found.next(), found.next()) {
            (Some(node), None) => node.clone(),
            _ => format!("test:{test}"),
        }
    }
}

fn north() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"))
}

fn test_nodes(north: &Path) -> Vec<String> {
    let query = r#"{"find":"r","rules":[{"head":{"rel":"r","args":[{"var":"s"}]},"body":[{"rel":"triple","args":[{"var":"s"},["keyword","in-binary"],{"var":"o"}]}]}]}"#;
    let Ok(output) = Command::new(north).args(["graph", "ask", "--json", query]).stdin(Stdio::null()).stderr(Stdio::null()).output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut nodes = Vec::new();
    collect_strings(&serde_json::from_str(&text).unwrap_or_default(), &mut nodes);
    nodes.retain(|node| node.starts_with("test:"));
    nodes
}

fn collect_strings(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => into.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, into)),
        serde_json::Value::Object(fields) => fields.values().for_each(|item| collect_strings(item, into)),
        _ => {}
    }
}

fn landed(ledger: &Path) -> BTreeMap<String, Vec<String>> {
    let text = fs::read_to_string(ledger).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let (sha, lanes) = (fields.get(2)?, fields.get(5)?);
            (!lanes.is_empty()).then(|| ((*sha).to_owned(), lanes.split(',').map(str::to_owned).collect()))
        })
        .collect()
}

pub(super) fn rev_parse(repo: &Path, revision: &str) -> Option<String> {
    git_lines(repo, &["rev-parse", "--verify", "--quiet", revision]).into_iter().next()
}

fn first_parents(repo: &Path, green: &str, red: &str) -> Vec<String> {
    git_lines(repo, &["rev-list", "--first-parent", "--reverse", &format!("{green}..{red}")])
}
