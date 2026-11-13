use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use super::profile::{Profile, Split, leaves, percentile};
use super::{Category, Span};
use crate::foundation::clock::Timestamp;
use crate::landing::check_timings::outcomes::{Outcome, Record};

pub struct Finish {
    pub wall: f64,
    pub wait: Option<f64>,
    pub build: f64,
    pub lint: f64,
    pub tests: f64,
    pub rest: f64,
    pub decided: String,
    pub rest_lines: Vec<(String, f64)>,
}

pub struct After {
    pub wall: f64,
    pub wait: f64,
    pub build: f64,
    pub lint: f64,
    pub tests: f64,
    pub rest: f64,
}

pub struct CheckTime {
    pub name: String,
    pub landed: f64,
    pub lander: f64,
    pub finish: Option<Finish>,
    pub host: Option<String>,
    pub after: Option<After>,
}

impl CheckTime {
    pub fn total(&self) -> Option<f64> {
        self.finish.as_ref().map(|finish| finish.wall + self.lander)
    }
}

pub struct Evidence<'a> {
    pub records: &'a [Record],
    pub heads: BTreeMap<String, String>,
}

/// The head of each lane queued in the queues behind `spans`, by ticket, from the queue's ticket files.
pub fn lane_heads(spans: &[PathBuf]) -> BTreeMap<String, String> {
    let mut heads = BTreeMap::new();
    for queue in spans.iter().filter_map(|file| file.parent()?.parent()) {
        for directory in [queue.to_path_buf(), queue.join("done")] {
            for entry in fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")) {
                let Some(stem) = entry.file_stem().and_then(|stem| stem.to_str()) else { continue };
                if let Some(head) = ticket_head(&entry) {
                    heads.insert(stem.to_owned(), head);
                }
            }
        }
    }
    heads
}

fn ticket_head(path: &Path) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&fs::read_to_string(path).ok()?).ok()?["head"].as_str().map(str::to_owned)
}

fn finish_rows<'a>(profile: &Profile<'_>, evidence: &'a Evidence<'_>) -> Vec<&'a Record> {
    let Some(head) = profile.landing.attribute("ticket").and_then(|ticket| evidence.heads.get(ticket)) else { return Vec::new() };
    #[allow(clippy::cast_precision_loss)]
    evidence.records.iter().filter(|record| record.outcome != Outcome::Cancelled && record.landing == *head && (record.started as f64) < profile.landing.start).collect()
}

/// The host that ran most of the landing's finish-step check, else its build, lint and test spans.
pub fn executed_on(profile: &Profile<'_>, evidence: &Evidence<'_>) -> Option<String> {
    let mut seconds: BTreeMap<String, f64> = BTreeMap::new();
    for record in finish_rows(profile, evidence) {
        if let Some(host) = &record.host {
            *seconds.entry(host.clone()).or_default() += record.wall;
        }
    }
    if seconds.is_empty() {
        for span in profile.spans.iter().filter(|span| !span.host.is_empty() && matches!(span.category, Category::Compile | Category::Lint | Category::Test)) {
            *seconds.entry(span.host.clone()).or_default() += span.seconds();
        }
    }
    seconds.into_iter().max_by(|left, right| left.1.total_cmp(&right.1)).map(|(host, _)| host)
}

fn finish(rows: &[&Record]) -> Option<Finish> {
    #[allow(clippy::cast_precision_loss)]
    let at = |record: &Record| record.started as f64;
    let start = rows.iter().map(|record| at(record)).reduce(f64::min)?;
    let end = rows.iter().map(|record| at(record) + record.wall).reduce(f64::max)?;
    let known = rows.iter().all(|record| record.waited.is_some());
    let mut spans = Vec::new();
    for record in rows {
        let run = Span::new("finish", record.command.clone(), Category::of_command(&record.command), at(record), at(record) + record.wall);
        let waited = record.waited.unwrap_or_default().min(record.wall);
        if waited > 0.0 {
            spans.push(Span::new("finish", "line wait", Category::HostWait, at(record), at(record) + waited).under(&run.id));
        }
        spans.push(run);
    }
    let split = leaves(&spans, (start, end));
    let decided = decided(rows);
    let mut rest_lines: Vec<(String, f64)> = rows.iter().filter(|record| !matches!(Category::of_command(&record.command), Category::Compile | Category::Lint | Category::Test)).map(|record| (shortened(&record.command), record.wall)).collect();
    rest_lines.sort_by(|left, right| right.1.total_cmp(&left.1));
    rest_lines.truncate(REST_LINES);
    Some(Finish { wall: end - start, wait: known.then(|| split.get(Category::HostWait)), build: split.get(Category::Compile), lint: split.get(Category::Lint), tests: split.get(Category::Test), rest: rest(&split), decided, rest_lines })
}

fn decided(rows: &[&Record]) -> String {
    let tests: Vec<&&Record> = rows.iter().filter(|record| record.command.contains("nextest")).collect();
    let split = tests.iter().filter_map(|record| record.crates).fold(None, |sum: Option<(u32, u32)>, crates| Some(sum.map_or((crates.decided, crates.tested), |(decided, tested)| (decided + crates.decided, tested + crates.tested))));
    match (split, tests.first()) {
        (Some((decided, tested)), _) => format!("Buck2 {decided} crates, declared line {tested}"),
        (None, Some(record)) if record.host.is_some() => "declared line (no Buck2 split)".to_owned(),
        (None, Some(_)) => "unknown (row before the host column)".to_owned(),
        (None, None) => "no nextest row".to_owned(),
    }
}

const REST_LINES: usize = 3;

fn shortened(command: &str) -> String {
    let command = command.trim_start_matches("CARGO_TARGET_DIR=target ").trim_start_matches("./bin/");
    command.chars().take(110).collect()
}

fn rest(split: &Split) -> f64 {
    let named = [Category::Queue, Category::HostWait, Category::Compile, Category::Lint, Category::Test];
    split.categories.iter().filter(|(category, _)| !named.contains(category)).map(|(_, seconds)| seconds).sum()
}

fn after(profile: &Profile<'_>) -> Option<After> {
    let split = &profile.after;
    (profile.after_wall > 0.0).then(|| After { wall: profile.after_wall, wait: split.get(Category::HostWait), build: split.get(Category::Compile), lint: split.get(Category::Lint), tests: split.get(Category::Test), rest: rest(split) })
}

pub fn of(profile: &Profile<'_>, evidence: &Evidence<'_>) -> CheckTime {
    CheckTime { name: profile.landing.name.clone(), landed: profile.landing.end, lander: profile.wall(), finish: finish(&finish_rows(profile, evidence)), host: executed_on(profile, evidence), after: after(profile) }
}

fn seconds(value: f64) -> String {
    format!("{:.0} s", value.max(0.0) + 0.0)
}

fn cell(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), seconds)
}

fn spread(values: Vec<f64>, fraction: f64) -> Option<f64> {
    (!values.is_empty()).then(|| percentile(&values, fraction))
}

fn row<const N: usize>(name: &str, landed: &str, cells: [String; N]) -> String {
    let mut line = format!("{name:<30} {landed:>11}");
    for cell in cells {
        let _ = write!(line, " {cell:>9}");
    }
    line
}

pub fn render(profiles: &[Profile<'_>], evidence: &Evidence<'_>, host: &str, since: f64) -> String {
    let times: Vec<CheckTime> = profiles.iter().map(|profile| of(profile, evidence)).collect();
    let mut out = String::new();
    let _ = writeln!(out, "north land profile --host {host}: the last {} landing{} whose checks ran on {host} since {}", times.len(), if times.len() == 1 { "" } else { "s" }, Timestamp::from_unix_seconds_f64(since).local_twelve_hour());
    if times.is_empty() {
        return out;
    }
    let _ = writeln!(out, "finish = the finish step's check in the lane before queueing, first check started to last ended, from the check record; lander = the lander's own number, enqueue to push; total = finish + lander and nothing else");
    let _ = writeln!(out, "line wait = the wait for a build-host slot and is never counted in build, lint, tests or rest; n/a = the record's row cannot tell line wait from run time (a row written before this change), so that wait is inside build, lint, tests or rest; tests = a whole nextest line, build and run together; - = no row; the host, line wait and crate split are measurement only: a lane's own test output can print the same lines, so no promotion or pacing decision may read them");
    let _ = writeln!(out, "\nthe finish step, then the lander:");
    let _ = writeln!(out, "{}  host   tests decided by", row("landing", "landed", ["total", "finish", "line wait", "build", "lint", "tests", "rest", "lander"].map(str::to_owned)));
    let wait = |finish: &Finish| if finish.wait.is_some() { cell(finish.wait) } else { "n/a".to_owned() };
    for time in &times {
        let finish = time.finish.as_ref();
        let cells = [cell(time.total()), cell(finish.map(|f| f.wall)), finish.map_or_else(|| "-".to_owned(), wait), cell(finish.map(|f| f.build)), cell(finish.map(|f| f.lint)), cell(finish.map(|f| f.tests)), cell(finish.map(|f| f.rest)), seconds(time.lander)];
        let _ = writeln!(out, "{}  {:<6} {}", row(&time.name.chars().take(30).collect::<String>(), &Timestamp::from_unix_seconds_f64(time.landed).local_twelve_hour(), cells), time.host.as_deref().unwrap_or("-"), finish.map_or("-", |f| f.decided.as_str()));
    }
    for (group, buck2) in [("Buck2", true), ("not Buck2", false)] {
        let members: Vec<&CheckTime> = times.iter().filter(|time| time.finish.as_ref().is_some_and(|finish| finish.decided.starts_with("Buck2")) == buck2).collect();
        if members.is_empty() {
            continue;
        }
        for (label, fraction) in [("p50", 0.5), ("p90", 0.9)] {
            let over = |value: &dyn Fn(&CheckTime) -> Option<f64>| spread(members.iter().copied().filter_map(value).collect(), fraction);
            let known_wait = members.iter().filter_map(|time| time.finish.as_ref()).filter(|finish| finish.wait.is_some()).count();
            let waits = if known_wait == 0 { "n/a".to_owned() } else { cell(over(&|time| time.finish.as_ref().and_then(|finish| finish.wait))) };
            let cells = [cell(over(&CheckTime::total)), cell(over(&|time| time.finish.as_ref().map(|f| f.wall))), waits, cell(over(&|time| time.finish.as_ref().map(|f| f.build))), cell(over(&|time| time.finish.as_ref().map(|f| f.lint))), cell(over(&|time| time.finish.as_ref().map(|f| f.tests))), cell(over(&|time| time.finish.as_ref().map(|f| f.rest))), cell(over(&|time| Some(time.lander)))];
            let _ = writeln!(out, "{}", row(&format!("{label} {group}"), "", cells));
        }
    }
    let _ = writeln!(out, "\nwhat is inside rest (the longest lines of the finish step that are not a build, a lint or a test run, by their own wall seconds; they overlap each other):");
    for time in &times {
        if let Some(finish) = &time.finish {
            let lines: Vec<String> = finish.rest_lines.iter().map(|(command, wall)| format!("{} {}", seconds(*wall), command)).collect();
            let _ = writeln!(out, "- {}: rest {}; {}", time.name, seconds(finish.rest), if lines.is_empty() { "no such line".to_owned() } else { lines.join("; ") });
        }
    }
    let _ = writeln!(out, "\nthe verification after the push:");
    let _ = writeln!(out, "{}", row("landing", "landed", ["after", "line wait", "build", "lint", "tests", "rest"].map(str::to_owned)));
    for time in &times {
        let cells = match &time.after {
            Some(after) => [after.wall, after.wait, after.build, after.lint, after.tests, after.rest].map(seconds),
            None => ["-"; 6].map(str::to_owned),
        };
        let _ = writeln!(out, "{}", row(&time.name.chars().take(30).collect::<String>(), &Timestamp::from_unix_seconds_f64(time.landed).local_twelve_hour(), cells));
    }
    for (label, fraction) in [("p50", 0.5), ("p90", 0.9)] {
        let over = |value: &dyn Fn(&After) -> f64| cell(spread(times.iter().filter_map(|time| time.after.as_ref()).map(value).collect(), fraction));
        let cells = [over(&|a| a.wall), over(&|a| a.wait), over(&|a| a.build), over(&|a| a.lint), over(&|a| a.tests), over(&|a| a.rest)];
        let _ = writeln!(out, "{}", row(label, "", cells));
    }
    out
}
