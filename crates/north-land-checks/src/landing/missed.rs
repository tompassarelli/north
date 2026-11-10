use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::foundation::check_log::{self, Failure};
use crate::foundation::clock::Timestamp;
use crate::land::slots::graph_selection::{SELECTED, Selected};
use crate::land::slots::guard_suites::Suites;
use crate::platform::records_mirror;

mod responsible;

use responsible::{Blame, responsible};

const USAGE: &str = "usage: north land missed --since DATE [--until DATE] [--repo DIR]
lists each failing Cargo run on main created at or after DATE and before --until (gh's --created form, such as 2026-11-09T01:00:00Z), its failed tests, and the ones the change graph's after-landing selection for the landing range holding that commit left out; for each failed test it names the lane that turned it red, and when that lane's own selection did not hold the test it records a selection-miss incident and tells the graph (test :guards FILE) for each file the lane changed; exits 1 when one was left out";
const CLASS: &str = "selection-miss";
const SETTLED: f64 = 3600.0;

#[derive(Deserialize)]
struct Run {
    #[serde(rename = "databaseId")]
    id: u64,
    #[serde(rename = "headSha")]
    sha: String,
    conclusion: String,
    #[serde(rename = "createdAt")]
    created: String,
}

struct Landing {
    base: String,
    head: String,
    selected: Selected,
}

struct Window {
    since: String,
    until: Option<String>,
}

impl Window {
    fn created(&self) -> String {
        self.until.as_ref().map_or_else(|| format!(">={}", self.since), |until| format!("{}..{until}", self.since))
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut since, mut until, mut repo) = (None, None, PathBuf::from("."));
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--since", Some(value)) => since = Some(value.clone()),
            ("--until", Some(value)) => until = Some(value.clone()),
            ("--repo", Some(value)) => repo = PathBuf::from(value),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let Some(since) = since else {
        eprintln!("{USAGE}");
        return 2;
    };
    let window = Window { since, until };
    let records = records_mirror::read("north land missed");
    if let Some(why) = refused(&records, &window.since, window.until.as_deref(), records_mirror::now()) {
        eprintln!("north land missed: {why}");
        return 1;
    }
    let landings = landings(&records.home().join("north/land-queue"));
    let suites = Suites::read(&repo).ok();
    let landed = landed(&records.home().join("safe-push/post-land.tsv"));
    let mut runs = match completed_runs(&repo, &window) {
        Ok(runs) => runs,
        Err(why) => {
            eprintln!("north land missed: {why}");
            return 1;
        }
    };
    runs.sort_by(|left, right| left.created.cmp(&right.created));
    let mut outcomes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let (mut judged, mut missed, mut unjudged, mut failing) = (0, 0, 0, Vec::new());
    for run in &runs {
        if run.conclusion != "failure" {
            outcomes.entry(run.sha.clone()).or_default();
            continue;
        }
        let tests: Vec<String> = log(&repo, run.id).map(|text| check_log::failures(&text).into_iter().filter_map(|failure| if let Failure::Cargo(name) = failure { Some(name) } else { None }).collect()).unwrap_or_default();
        outcomes.entry(run.sha.clone()).or_default().extend(tests.iter().cloned());
        failing.push((run, tests.clone()));
        if tests.is_empty() {
            println!("run {} at {}: its log names no failed test", run.id, short(&run.sha));
            continue;
        }
        let Some(landing) = landings.iter().find(|landing| holds(&repo, landing, &run.sha)) else {
            unjudged += tests.len();
            println!("run {} at {}: {} failed tests, and no after-landing run recorded a change-graph selection for a range holding it: {}", run.id, short(&run.sha), tests.len(), tests.join(", "));
            continue;
        };
        let floor = suites.as_ref().map(|suites| suites.floor(&changed(&repo, &landing.base, &landing.head)));
        let held = |name: &str| landing.selected.holds(name) || floor.as_ref().is_some_and(|floor| floor.selects(name));
        let guarded = tests.iter().map(String::as_str).filter(|name| !landing.selected.holds(name) && floor.as_ref().is_some_and(|floor| floor.selects(name))).count();
        let left: Vec<&str> = tests.iter().map(String::as_str).filter(|name| !held(name)).collect();
        judged += tests.len();
        missed += left.len();
        let detail = if !left.is_empty() {
            format!("; left out: {}", left.join(", "))
        } else if guarded > 0 {
            format!("; {guarded} held by the guard suite")
        } else {
            String::new()
        };
        println!("run {} at {}: {} failed tests, {} in the after-landing selection of {}..{}{detail}", run.id, short(&run.sha), tests.len(), tests.len() - left.len(), short(&landing.base), short(&landing.head));
    }
    let failing_count = failing.len();
    let mut blamed = BTreeSet::new();
    let mut tellers = Tellers { north: north(), repo: &repo, nodes: None };
    for (run, tests) in failing {
        let Some(green) = runs.iter().rev().find(|earlier| earlier.created < run.created && earlier.conclusion == "success" && ancestor(&repo, &earlier.sha, &run.sha)) else {
            println!("run {} at {}: no Cargo run in the window passed before it, so no lane is named", run.id, short(&run.sha));
            continue;
        };
        let chain = first_parents(&repo, &green.sha, &run.sha);
        for test in tests {
            match responsible(&chain, &outcomes, &landed, &test) {
                Blame::Unjudged(why) => println!("{test} at {}: {why}", short(&run.sha)),
                Blame::Lane { lane, sha } => {
                    if blamed.insert((lane.clone(), test.clone())) {
                        tellers.judge(&landings, &lane, &sha, &test, run);
                    }
                }
            }
        }
    }
    println!("north land missed: {failing_count} failing Cargo runs on main since {}; of {judged} failed tests in a range with a recorded selection, {missed} were left out of it; {unjudged} failed in a range with none", window.since);
    u8::from(missed > 0)
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

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn refused(records: &records_mirror::Records, since: &str, until: Option<&str>, now: f64) -> Option<String> {
    let stamp = records.source()?;
    let age = (now - stamp.at).max(0.0);
    let at = |text: &str| Timestamp::parse_rfc3339(text).map(|at| at.unix_seconds_f64());
    let before = at(since).is_some_and(|since| stamp.at < since);
    let settled = until.and_then(at).is_some_and(|until| stamp.at >= until + SETTLED);
    if !before && (settled || age <= records_mirror::FRESH.as_secs_f64()) {
        return None;
    }
    let note = records.note(now).unwrap_or_default();
    Some(format!("refusing a stale mirror: {note}; only {} holds the land records this window judges, so run it there or end the window with --until an hour before the mirror", stamp.host))
}
fn landings(queues: &Path) -> Vec<Landing> {
    let logs = fs::read_dir(queues).into_iter().flatten().flatten().map(|queue| queue.path().join("logs"));
    let outcomes = logs.flat_map(|logs| fs::read_dir(logs).into_iter().flatten().flatten().map(|entry| entry.path())).filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("post-land-")) && path.extension().is_some_and(|extension| extension == "outcome"));
    let mut found = Vec::new();
    for text in outcomes.filter_map(|path| fs::read_to_string(path).ok()) {
        for line in text.lines() {
            let fields: Vec<&str> = line.splitn(4, '\t').collect();
            if let [SELECTED, base, head, json] = fields.as_slice()
                && let Ok(selected) = serde_json::from_str(json)
            {
                found.push(Landing { base: (*base).to_owned(), head: (*head).to_owned(), selected });
            }
        }
    }
    found
}

fn ancestor(repo: &Path, older: &str, newer: &str) -> bool {
    Command::new("git").arg("-C").arg(repo).args(["merge-base", "--is-ancestor", older, newer]).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn holds(repo: &Path, landing: &Landing, sha: &str) -> bool {
    sha != landing.base && ancestor(repo, &landing.base, sha) && ancestor(repo, sha, &landing.head)
}

fn completed_runs(repo: &Path, window: &Window) -> Result<Vec<Run>, String> {
    let output = Command::new("gh").args(["run", "list", "--branch", "main", "--workflow", "Cargo", "--status", "completed", "--limit", "1000", "--json", "databaseId,headSha,conclusion,createdAt", "--created", &window.created()]).current_dir(repo).stdin(Stdio::null()).output().map_err(|why| format!("gh run list: {why}"))?;
    if !output.status.success() {
        return Err(format!("gh run list: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let runs: Vec<Run> = serde_json::from_slice(&output.stdout).map_err(|why| format!("gh run list: {why}"))?;
    Ok(runs.into_iter().filter(|run| run.conclusion == "success" || run.conclusion == "failure").collect())
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

fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).stderr(Stdio::null()).output();
    output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()).unwrap_or_default()
}

fn rev_parse(repo: &Path, revision: &str) -> Option<String> {
    git_lines(repo, &["rev-parse", "--verify", "--quiet", revision]).into_iter().next()
}

fn first_parents(repo: &Path, green: &str, red: &str) -> Vec<String> {
    git_lines(repo, &["rev-list", "--first-parent", "--reverse", &format!("{green}..{red}")])
}

fn changed(repo: &Path, parent: &str, sha: &str) -> Vec<String> {
    git_lines(repo, &["diff", "--name-only", "--no-renames", parent, sha])
}
fn log(repo: &Path, run: u64) -> Option<String> {
    let output = Command::new("gh").args(["run", "view", &run.to_string(), "--log-failed"]).current_dir(repo).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "missed_tests.rs"]
mod tests;
