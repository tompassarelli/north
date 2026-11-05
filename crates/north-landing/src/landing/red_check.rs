use crate::platform::process;
use crate::platform::rolling_issue::{self, Breaches, Tracker, alarm, section_box};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: north land-red-check --main SHA --log FILE [--lane BRANCH] | north land-red-wait --main SHA [--timeout SECONDS]";
const GH_LIMIT: Duration = Duration::from_secs(60);
const OFFLOAD_SECONDS: u64 = 1800;
const RANGE_LIMIT: usize = 200;
pub const MAIN_RED: u8 = 3;
const UNKNOWN: u8 = 2;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Failure {
    Cargo(String),
    Harness(String),
    Line(String),
}

impl Failure {
    fn key(&self) -> String {
        match self {
            Failure::Cargo(name) => format!("cargo {name}"),
            Failure::Harness(path) => format!("harness {path}"),
            Failure::Line(text) => format!("FAIL {text}"),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Failure::Cargo(name) | Failure::Harness(name) | Failure::Line(name) => name,
        }
    }
}

fn log_text(line: &str) -> &str {
    let line = line.rsplit('\t').next().unwrap_or(line);
    match line.split_once(' ') {
        Some((stamp, rest)) if stamp.len() >= 20 && stamp.as_bytes()[4] == b'-' && stamp.ends_with('Z') => rest,
        _ => line,
    }
    .trim()
}

pub fn failures(log: &str) -> BTreeSet<Failure> {
    let mut found = BTreeSet::new();
    for line in log.lines().map(log_text) {
        if let Some(name) = line.strip_prefix("---- ").and_then(|rest| rest.strip_suffix(" stdout ----")) {
            found.insert(Failure::Cargo(name.trim().to_owned()));
        } else if let Some(name) = line.strip_prefix("test ").and_then(|rest| rest.strip_suffix(" ... FAILED")) {
            found.insert(Failure::Cargo(name.trim().to_owned()));
        } else if let Some(rest) = line.strip_prefix("land check: ") {
            let Some((path, verdict)) = rest.split_once(' ') else { continue };
            if (path.ends_with(".sh") || path.ends_with(".py")) && (verdict.starts_with("exited") || verdict.starts_with("was killed")) {
                found.insert(Failure::Harness(path.to_owned()));
            }
        } else if let Some(text) = line.strip_prefix("FAIL ").or_else(|| line.strip_prefix("FAIL: ")) {
            found.insert(Failure::Line(text.trim().to_owned()));
        }
    }
    found
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

pub fn rerun_command(failed: &BTreeSet<Failure>) -> Option<String> {
    let cargo: Vec<String> = failed.iter().filter_map(|failure| match failure {
        Failure::Cargo(name) => Some(quote(name)),
        _ => None,
    }).collect();
    let harness: Vec<&str> = failed.iter().filter_map(|failure| match failure {
        Failure::Harness(path) => Some(path.as_str()),
        _ => None,
    }).collect();
    if cargo.is_empty() && harness.is_empty() {
        return None;
    }
    let mut steps = vec!["status=0".to_owned()];
    if !cargo.is_empty() {
        steps.push(format!(
            "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo test --locked --offline --workspace --no-fail-fast -- --exact {} 2>&1 || status=1",
            cargo.join(" ")
        ));
    }
    for path in harness {
        let interpreter = if path.ends_with(".py") { "python3" } else { "bash" };
        let path = quote(path);
        steps.push(format!("{interpreter} {path} 2>&1 || {{ echo \"land check: \"{path}\" exited 1\"; status=1; }}"));
    }
    steps.push("exit $status".to_owned());
    Some(steps.join("; "))
}

struct Request {
    main: String,
    log: PathBuf,
    lane: String,
}

fn flag(arguments: &[String], name: &str) -> Option<String> {
    arguments.chunks(2).find(|pair| pair[0] == name).and_then(|pair| pair.get(1)).cloned()
}

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn gh(arguments: &[&str]) -> Option<String> {
    let output = process::run_within("gh", arguments, GH_LIMIT)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn state_directory() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
    });
    state.join("safe-push/red-check")
}

fn record_path(tree: &str) -> PathBuf {
    state_directory().join(format!("{tree}.tsv"))
}

fn breaches_path() -> PathBuf {
    state_directory().join("breaches.json")
}

fn load_breaches() -> Breaches {
    fs::read_to_string(breaches_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save_breaches(breaches: &Breaches) {
    let path = breaches_path();
    if path.parent().is_some_and(|parent| fs::create_dir_all(parent).is_ok())
        && let Ok(text) = serde_json::to_string(breaches)
    {
        let _ = fs::write(path, text + "\n");
    }
}

fn recorded(tree: &str) -> BTreeMap<String, bool> {
    fs::read_to_string(record_path(tree))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(verdict, key)| (key.to_owned(), verdict == "fail"))
        .collect()
}

fn record(tree: &str, verdicts: &BTreeMap<String, bool>) {
    let path = record_path(tree);
    let mut all = recorded(tree);
    all.extend(verdicts.iter().map(|(key, failed)| (key.clone(), *failed)));
    let body: String = all.iter().map(|(key, failed)| format!("{}\t{key}\n", if *failed { "fail" } else { "pass" })).collect();
    if path.parent().is_some_and(|parent| fs::create_dir_all(parent).is_ok()) {
        let _ = fs::write(path, body);
    }
}

fn verdicts_on_main(main: &str, tree: &str, failed: &BTreeSet<Failure>) -> Option<(BTreeMap<String, bool>, &'static str)> {
    let known = recorded(tree);
    if failed.iter().all(|failure| known.contains_key(&failure.key())) {
        return Some((failed.iter().map(|failure| (failure.key(), known[&failure.key()])).collect(), "recorded result"));
    }
    let command = rerun_command(failed)?;
    let host = std::env::var("NORTH_RED_CHECK_HOST").unwrap_or_else(|_| "gex45".to_owned());
    println!("red check: running the {} failing test(s) on main's own tree {} on {host}", failed.len(), &main[..main.len().min(12)]);
    let output = Command::new("north")
        .args(["capacity", "offload", "run", "--host", &host, "--base", main, "--head", main, "--timeout", &OFFLOAD_SECONDS.to_string(), "--", &command])
        .output()
        .ok()?;
    let log = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let on_main = failures(&log);
    if !output.status.success() && on_main.is_empty() {
        eprintln!("red check: the run on main's tree failed without naming a test");
        return None;
    }
    let verdicts: BTreeMap<String, bool> = failed.iter().map(|failure| (failure.key(), on_main.contains(failure))).collect();
    record(tree, &verdicts);
    Some((verdicts, "run on gex45"))
}

fn green_commits() -> BTreeSet<String> {
    let mut green = BTreeSet::new();
    for workflow in ["cargo.yml", "harness.yml"] {
        let Some(runs) = gh(&["run", "list", "--workflow", workflow, "--branch", "main", "--status", "success", "--limit", "50", "--json", "headSha"]) else { continue };
        let runs: Value = serde_json::from_str(&runs).unwrap_or(Value::Null);
        green.extend(runs.as_array().into_iter().flatten().filter_map(|run| run.get("headSha")?.as_str().map(str::to_owned)));
    }
    green
}

fn test_file(failure: &Failure) -> Option<String> {
    match failure {
        Failure::Harness(path) => Some(path.clone()),
        Failure::Cargo(name) => {
            let leaf = name.rsplit("::").next()?;
            let found = git(&["grep", "-l", "-E", &format!(r"fn {leaf}\b"), "--", "*.rs"])?;
            found.lines().next().map(str::to_owned)
        }
        Failure::Line(_) => None,
    }
}

fn crate_root(file: &str) -> String {
    let mut directory = Path::new(file).parent();
    while let Some(candidate) = directory {
        if candidate.as_os_str().is_empty() {
            break;
        }
        if candidate.join("Cargo.toml").is_file() {
            return format!("{}/", candidate.display());
        }
        directory = candidate.parent();
    }
    "src/".to_owned()
}

pub fn quoted_paths(source: &str) -> BTreeSet<String> {
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .map(|literal| literal.trim_start_matches('/'))
        .filter(|literal| literal.contains('/') && !literal.contains(' ') && literal.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)))
        .map(str::to_owned)
        .collect()
}

pub fn test_inputs(failed: &BTreeSet<Failure>) -> Vec<String> {
    let mut inputs = BTreeSet::new();
    for file in failed.iter().filter_map(test_file) {
        if file.ends_with(".rs") {
            inputs.insert(crate_root(&file));
        }
        let source = fs::read_to_string(&file).unwrap_or_default();
        inputs.extend(quoted_paths(&source).into_iter().filter(|path| Path::new(path).exists()));
        inputs.insert(file);
    }
    inputs.into_iter().collect()
}

fn culprit(main: &str, failed: &BTreeSet<Failure>) -> String {
    let green = green_commits();
    let history = git(&["rev-list", "--first-parent", &format!("--max-count={RANGE_LIMIT}"), main]).unwrap_or_default();
    let last_green = history.lines().find(|sha| green.contains(*sha));
    let range = last_green.map_or_else(|| main.to_owned(), |sha| format!("{sha}..{main}"));
    let mut arguments = vec!["log".to_owned(), "-1".to_owned(), "--format=%h %s".to_owned(), range, "--".to_owned()];
    let inputs = test_inputs(failed);
    arguments.extend(inputs.iter().cloned());
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let found = (!inputs.is_empty()).then(|| git(&words)).flatten().filter(|line| !line.is_empty());
    let since = last_green.map_or_else(|| "no green main run found".to_owned(), |sha| format!("since green {}", &sha[..sha.len().min(9)]));
    match found {
        Some(line) => format!("{line} ({since})"),
        None => {
            let newest = git(&["log", "-1", "--format=%h %s", main]).unwrap_or_default();
            format!("{newest} (newest main commit; none {since} changed the tests' inputs)")
        }
    }
}

pub fn event_line(main: &str, red: &[&Failure], culprit: &str, issue: Option<&str>) -> String {
    let mut names: Vec<&str> = red.iter().take(3).map(|failure| failure.label()).collect();
    if red.len() > 3 {
        names.push("…");
    }
    let marker = issue.map_or_else(|| "Main-red:".to_owned(), |number| format!("Main-red: #{number}"));
    format!(
        "main is red at {}: {} fails on main's own tree; likely culprit {culprit}; a fix lane carrying {marker} lands first and blocked lanes retry after it",
        &main[..main.len().min(9)],
        names.join(", ")
    )
}

const ISSUE_BODY: &str = "Main CI failures and confirmed culprit decisions.\n\n## Done when\n- [ ] The newest Harness and Cargo runs on main are green on a commit after the newest culprit range below: `gh run list --branch main --workflow Harness --limit 1` and the same for Cargo both print success; close this issue with that run's link.";

fn repository() -> Option<String> {
    gh(&["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"]).map(|name| name.trim().to_owned()).filter(|name| !name.is_empty())
}

pub const ISSUE_TITLE: &str = "main is red";
const SECTION: &str = "## Red mains";

pub fn record_red(tracker: &mut dyn Tracker, breaches: &mut Breaches, main: &str, line: &str) -> Result<Option<u64>, String> {
    let base = tracker.open(ISSUE_TITLE)?.map_or_else(|| ISSUE_BODY.to_owned(), |issue| issue.body);
    let key = format!("main is red at {}:", &main[..main.len().min(9)]);
    let body = section_box(&base, SECTION, &key, &format!("- [ ] {line}"), true);
    alarm(tracker, breaches, ISSUE_TITLE, Some(&body), "")?;
    Ok(tracker.open(ISSUE_TITLE)?.map(|issue| issue.number))
}

pub fn record_green(breaches: &mut Breaches) {
    breaches.record(ISSUE_TITLE, false);
}

fn lead() -> Option<String> {
    let session = std::env::var("NORTH_FLEET_SESSION").ok().filter(|name| !name.is_empty())?;
    let output = Command::new("north").args(["fleet", "list", "--json"]).output().ok()?;
    let rows: Value = serde_json::from_slice(&output.stdout).ok()?;
    rows.as_array()?
        .iter()
        .find(|row| row.get("name").and_then(Value::as_str) == Some(session.as_str()))?
        .get("parent")?
        .as_str()
        .map(str::to_owned)
}

fn send(target: &str, line: &str) {
    let sent = Command::new("north").args(["fleet", "send", "--from", "service:north-land-red-check", target, line]).output().is_ok_and(|output| output.status.success());
    if !sent {
        eprintln!("red check: could not send the event to {target}");
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let (Some(main), Some(log)) = (flag(arguments, "--main"), flag(arguments, "--log")) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let request = Request { main, log: PathBuf::from(log), lane: flag(arguments, "--lane").unwrap_or_default() };
    let failed = failures(&fs::read_to_string(&request.log).unwrap_or_default());
    if failed.is_empty() {
        println!("red check: the failed log names no test; treating it as this lane's failure");
        return UNKNOWN;
    }
    let Some(tree) = git(&["rev-parse", &format!("{}^{{tree}}", request.main)]) else {
        eprintln!("red check: cannot read main's tree {}", request.main);
        return UNKNOWN;
    };
    let Some((verdicts, source)) = verdicts_on_main(&request.main, &tree, &failed) else {
        return UNKNOWN;
    };
    let red: Vec<&Failure> = failed.iter().filter(|failure| verdicts.get(&failure.key()).copied().unwrap_or(false)).collect();
    let mut breaches = load_breaches();
    if red.is_empty() {
        record_green(&mut breaches);
        save_breaches(&breaches);
        println!("red check: {} failing test(s) pass on main's own tree ({source}); this lane broke them", failed.len());
        return 0;
    }
    let culprit = culprit(&request.main, &failed);
    let recorded = repository().ok_or_else(|| "gh repo view names no repository".to_owned()).and_then(|repo| {
        record_red(&mut rolling_issue::Gh::of(&repo, &[]), &mut breaches, &request.main, &event_line(&request.main, &red, &culprit, None))
    });
    let issue = match recorded {
        Ok(issue) => issue.map(|number| number.to_string()),
        Err(error) => {
            eprintln!("red check: the main is red issue is unsettled: {error}");
            None
        }
    };
    save_breaches(&breaches);
    let line = event_line(&request.main, &red, &culprit, issue.as_deref());
    let line = if request.lane.is_empty() { line } else { format!("{line} (seen landing {})", request.lane) };
    send("proxy", &line);
    if let Some(lead) = lead() {
        send(&lead, &line);
    }
    println!("red check: {line} ({source})");
    MAIN_RED
}

pub fn fix_landed(log: &str) -> bool {
    log.lines().any(|line| line.strip_prefix("Main-red:").is_some_and(|value| !value.trim().is_empty()))
}

pub fn wait(arguments: &[String]) -> u8 {
    let Some(main) = flag(arguments, "--main") else {
        eprintln!("{USAGE}");
        return 64;
    };
    let limit = Duration::from_secs(flag(arguments, "--timeout").and_then(|value| value.parse().ok()).unwrap_or(21_600));
    let poll = Duration::from_secs(std::env::var("NORTH_RED_WAIT_POLL").ok().and_then(|value| value.parse().ok()).unwrap_or(30));
    let started = Instant::now();
    println!("red check: waiting for a lane carrying Main-red: to land on origin main after {}", &main[..main.len().min(9)]);
    loop {
        let fetched = git(&["fetch", "-q", "origin", "main"]).is_some();
        let landed = fetched && git(&["log", "--format=%B", &format!("{main}..FETCH_HEAD")]).is_some_and(|log| fix_landed(&log));
        if landed {
            println!("red check: a Main-red: lane landed; retrying this lane");
            return 0;
        }
        if started.elapsed() >= limit {
            eprintln!("red check: no Main-red: lane landed within {} s", limit.as_secs());
            return 1;
        }
        thread::sleep(poll);
    }
}

#[cfg(test)]
#[path = "red_check_tests.rs"]
mod tests;
