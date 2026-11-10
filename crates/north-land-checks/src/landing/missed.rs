use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::foundation::clock::Timestamp;
use crate::land::slots::graph_selection::{SELECTED, Selected};
use crate::land::slots::guard_suites::Suites;
use crate::platform::records_mirror;

mod base;
mod flaky;
mod full_runs;
use flaky::Seen;
use full_runs::{Conclusion, Run, Window, failed_tests};
pub use base::classify;
mod responsible;
mod selection_miss;

pub const USAGE: &str = "usage: north land missed --since DATE_OR_SPAN [--until DATE] [--repo DIR] [--coverage FILE]
lists each failing full run on main from two sources, the Cargo runs of GitHub Actions created at or after DATE and before --until (gh's --created form, such as 2026-11-09T01:00:00Z) and the `north test coverage record` full run whose map (--coverage FILE, by default this host's map of the repository) was recorded in that window and names its failed tests, judged as a Cargo run on the map's commit; for each, its failed tests, and the ones the change graph's after-landing selection for the landing range holding that commit left out, apart from a test that also failed in the nearest earlier full run the commit descends from, which is listed as an environment failure, and a test that a later full run passed with no change between the two runs touching its crate or the crates it depends on, which is listed as flaky; neither is a miss; for each failed test it names the lane that turned it red, and when that lane's own selection did not hold the test it records a selection-miss incident and tells the graph (test :guards FILE) for each file the lane changed; exits 1 when one was left out";
const SETTLED: f64 = 3600.0;

struct Landing {
    base: String,
    head: String,
    selected: Selected,
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut since, mut until, mut repo, mut coverage) = (None, None, PathBuf::from("."), None);
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--coverage", Some(value)) => coverage = Some(PathBuf::from(value)),
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
    let window = Window { since: since_instant(&since, Timestamp::now()), until };
    let records = records_mirror::read("north land missed");
    if let Some(why) = refused(&records, &window.since, window.until.as_deref(), records_mirror::now()) {
        eprintln!("north land missed: {why}");
        return 1;
    }
    let landings = landings(&records.home().join("north/land-queue"));
    let suites = Suites::read(&repo).ok();
    let runs = match full_runs::gathered(&repo, &window, coverage) {
        Ok(runs) => runs,
        Err(why) => {
            eprintln!("north land missed: {why}");
            return 1;
        }
    };
    let mut outcomes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut seen: Vec<Seen> = Vec::new();
    for run in &runs {
        let tests = if run.conclusion == Conclusion::Failure { failed_tests(&repo, &run.source) } else { Vec::new() };
        outcomes.entry(run.sha.clone()).or_default().extend(tests.iter().cloned());
        seen.push(Seen { sha: run.sha.clone(), failed: tests.iter().cloned().collect(), names_its_failures: run.conclusion == Conclusion::Success || !tests.is_empty() });
    }
    let (mut tally, mut failing) = (Tally::default(), Vec::new());
    for (at, run) in runs.iter().enumerate().filter(|(_, run)| run.conclusion == Conclusion::Failure) {
        let tests: Vec<String> = seen[at].failed.iter().cloned().collect();
        failing.push((run, tests.clone()));
        if tests.is_empty() {
            println!("{} at {}: its log names no failed test", run.source, short(&run.sha));
            continue;
        }
        let on_base = flaky::before(&repo, &seen[..at], &run.sha).filter(|base| base.names_its_failures).map(|base| base.failed.iter().cloned().collect::<Vec<_>>());
        let reaches_nothing = |name: &str| flaky::reaches_nothing(&repo, &seen, at, name);
        tally.add(after_landing(&repo, &landings, suites.as_ref(), run, &tests, on_base.as_deref(), &reaches_nothing));
    }
    let failing_count = failing.len();
    selection_miss::name_lanes(&repo, &runs, failing, &outcomes, &landings, &records.home().join("safe-push/post-land.tsv"));
    tally.print_lists();
    println!("north land missed: {failing_count} failing full runs on main since {}; of {} failed tests in a range with a recorded selection, {} were left out of it; {} failed with no change reaching them and are flaky, not selection gaps; {} more also fail on their base run and are environment failures; {} failed in a range with none", window.since, tally.judged, tally.left_out.len(), tally.flaky.len(), tally.environment, tally.unjudged);
    u8::from(!tally.left_out.is_empty())
}

#[derive(Default)]
struct Tally {
    judged: usize,
    left_out: Vec<(String, String)>,
    flaky: Vec<(String, String)>,
    unjudged: usize,
    environment: usize,
}

impl Tally {
    fn add(&mut self, other: Self) {
        self.judged += other.judged;
        self.left_out.extend(other.left_out);
        self.flaky.extend(other.flaky);
        self.unjudged += other.unjudged;
        self.environment += other.environment;
    }

    fn print_lists(&self) {
        for (title, list) in [("selection gaps, left out of a selection that a change reached", &self.left_out), ("flaky, failed with no change reaching them", &self.flaky)] {
            let mut by_test: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for (test, sha) in list {
                by_test.entry(test).or_default().push(sha);
            }
            println!("{title}: {}", by_test.len());
            for (test, shas) in by_test {
                println!("  {test} at {}", shas.join(", "));
            }
        }
    }
}

fn after_landing(repo: &Path, landings: &[Landing], suites: Option<&Suites>, run: &Run, tests: &[String], on_base: Option<&[String]>, flaky: &dyn Fn(&str) -> bool) -> Tally {
    let Some(landing) = landings.iter().find(|landing| holds(repo, landing, &run.sha)) else {
        println!("{} at {}: {} failed tests, and no after-landing run recorded a change-graph selection for a range holding it: {}", run.source, short(&run.sha), tests.len(), tests.join(", "));
        return Tally { unjudged: tests.len(), ..Tally::default() };
    };
    let floor = suites.map(|suites| suites.floor(&changed(repo, &landing.base, &landing.head)));
    let guards = |name: &str| floor.as_ref().is_some_and(|floor| floor.selects(name));
    let verdict = classify(tests, on_base, flaky, |name| landing.selected.holds(name) || guards(name));
    let guarded = verdict.selected.iter().filter(|name| !landing.selected.holds(name)).count();
    let mut detail = String::new();
    if !verdict.left_out.is_empty() {
        detail.push_str(&format!("; left out: {}", verdict.left_out.join(", ")));
    } else if guarded > 0 {
        detail.push_str(&format!("; {guarded} held by the guard suite"));
    }
    if !verdict.flaky.is_empty() {
        detail.push_str(&format!("; flaky, no change reached them: {}", verdict.flaky.join(", ")));
    }
    if !verdict.environment.is_empty() {
        detail.push_str(&format!("; environment failures that also fail on the base: {}", verdict.environment.join(", ")));
    }
    println!("{} at {}: {} failed tests, {} in the after-landing selection of {}..{}{detail}", run.source, short(&run.sha), tests.len(), verdict.selected.len(), short(&landing.base), short(&landing.head));
    let at = |names: Vec<String>| names.into_iter().map(|name| (name, short(&run.sha).to_owned())).collect();
    Tally { judged: verdict.selected.len() + verdict.left_out.len() + verdict.flaky.len(), left_out: at(verdict.left_out), flaky: at(verdict.flaky), unjudged: 0, environment: verdict.environment.len() }
}

fn since_instant(text: &str, now: Timestamp) -> String {
    let Some((amount, unit)) = text.split_at_checked(text.len().saturating_sub(1)) else { return text.to_owned() };
    let scale = match unit {
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return text.to_owned(),
    };
    match amount.parse::<i64>() {
        Ok(count) if count > 0 => Timestamp::from_unix_seconds(now.unix_seconds() - count * scale).iso(),
        _ => text.to_owned(),
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

fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).stderr(Stdio::null()).output();
    output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()).unwrap_or_default()
}

fn changed(repo: &Path, parent: &str, sha: &str) -> Vec<String> {
    git_lines(repo, &["diff", "--name-only", "--no-renames", parent, sha])
}

#[cfg(test)]
#[path = "missed_tests.rs"]
mod tests;
