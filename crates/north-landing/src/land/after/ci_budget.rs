use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::foundation::clock::Timestamp;
use serde_json::Value;

use crate::land::scans::git;

const HOLD_SECONDS: i64 = 1800;
const RUNS_MEASURED: usize = 3;

pub struct Budget {
    workflow: String,
    seconds: f64,
    ceiling: Option<f64>,
    written: (String, String),
}

impl Budget {
    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (workflow, written, ceiling) = match fields.as_slice() {
            [workflow, seconds] => (*workflow, (*seconds, ""), None),
            [workflow, seconds, ceiling] => (*workflow, (*seconds, *ceiling), Some(number(ceiling)?)),
            _ => return None,
        };
        let seconds = number(written.0)?;
        let written = (written.0.to_owned(), written.1.to_owned());
        ceiling.is_none_or(|ceiling| ceiling > seconds).then(|| Self { workflow: workflow.to_owned(), seconds, ceiling, written })
    }
}

fn number(text: &str) -> Option<f64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, "0"));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    (digits(whole) && digits(fraction)).then(|| text.parse().ok()).flatten()
}

fn speed_up(line: &str, workflow: &str) -> bool {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [name, before, "->", after] = fields.as_slice() else { return false };
    let seconds = |field: &str| field.strip_suffix('s').and_then(number);
    *name == workflow && matches!((seconds(before), seconds(after)), (Some(before), Some(after)) if before > after)
}

pub struct Scan<'a> {
    pub tree: &'a Path,
    pub state: PathBuf,
}

struct Landing {
    changed: Vec<String>,
    commits: Vec<String>,
    main_red: Option<String>,
}

enum Verdict {
    Pass,
    Refuse,
}

impl Scan<'_> {
    pub fn run(&self, range: &str, out: &mut impl Write) -> u8 {
        let Some(budgets) = git::text(self.tree, &["config", "--blob", "refs/remotes/origin/main:.safe-push", "--get-all", "ci.budget"]) else {
            return 0;
        };
        let Some((base, head)) = range.split_once("..").filter(|(base, _)| !base.is_empty()) else {
            return 0;
        };
        let Some(landing) = self.landing(&git::merge_base(self.tree, base, head), head, out) else {
            return 0;
        };
        for line in budgets.lines() {
            let Some(budget) = Budget::parse(line) else {
                let _ = writeln!(out, "after-landing: ✗ invalid ci.budget in main’s .safe-push — expected <workflow-file> <seconds> [<ceiling seconds>].");
                return 1;
            };
            if let Verdict::Refuse = self.judge(&budget, &landing, out) {
                return 1;
            }
        }
        0
    }

    fn landing(&self, base: &str, head: &str, out: &mut impl Write) -> Option<Landing> {
        let names = git::bytes(self.tree, &["diff", "--name-only", "-z", "--no-renames", base, head]).unwrap_or_default();
        let changed: Vec<String> = names.split(|byte| *byte == 0).filter(|name| !name.is_empty()).map(|name| String::from_utf8_lossy(name).into_owned()).collect();
        if !changed.is_empty() && changed.iter().all(|path| path.starts_with(".github/")) {
            let _ = writeln!(out, "after-landing: CI budget exemption — every changed path is under .github/.");
            return None;
        }
        if !changed.is_empty() && changed.iter().all(|path| path.ends_with(".md")) {
            let _ = writeln!(out, "after-landing: CI budget exemption — every changed path is Markdown.");
            return None;
        }
        let log = git::text(self.tree, &["log", "--format=%H", &format!("{base}..{head}")]).unwrap_or_default();
        let commits: Vec<String> = log.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect();
        if commits.is_empty() {
            return None;
        }
        let main_red = self.main_red(&commits);
        Some(Landing { changed, commits, main_red })
    }

    fn trailer(&self, commit: &str, key: &str) -> String {
        git::text(self.tree, &["show", "-s", &format!("--format=%(trailers:key={key},valueonly)"), commit]).unwrap_or_default()
    }

    fn main_red(&self, commits: &[String]) -> Option<String> {
        let mut named = None;
        for commit in commits {
            let red = self.trailer(commit, "Main-red").split_whitespace().next()?.to_owned();
            if named.as_ref().is_some_and(|named| *named != red) {
                return None;
            }
            named = Some(red);
        }
        named
    }

    fn judge(&self, budget: &Budget, landing: &Landing, out: &mut impl Write) -> Verdict {
        let workflow = budget.workflow.as_str();
        if !super::workflow_paths::triggered(self.tree, workflow, &landing.changed) {
            let _ = writeln!(out, "after-landing: CI budget {workflow} exemption — no changed path matches its push paths filter.");
            return Verdict::Pass;
        }
        if landing.commits.iter().all(|commit| self.trailer(commit, "CI-budget").lines().any(|line| speed_up(line, workflow))) {
            let _ = writeln!(out, "after-landing: CI budget {workflow} exemption — every commit records a measured speed-up.");
            return Verdict::Pass;
        }
        let since = self.speed_up_on_main(workflow);
        let history = match workflow.strip_prefix("local:") {
            Some(source) => self.local_history(workflow, source, landing.main_red.as_deref(), out),
            None => self.remote_history(workflow, since.as_deref(), landing.main_red.as_deref(), out),
        };
        match history {
            Measured::Runs(history) => self.weigh(budget, since.as_deref(), &history, out),
            Measured::Exempt => Verdict::Pass,
            Measured::Invalid => Verdict::Refuse,
            Measured::Unmeasured(reason) => self.hold(workflow, &reason, None, out),
        }
    }

    fn speed_up_on_main(&self, workflow: &str) -> Option<String> {
        let log = git::text(self.tree, &["log", "refs/remotes/origin/main", "-n", "500", "-z", "--format=%H%n%B"])?;
        log.split('\0').find_map(|record| {
            let mut lines = record.lines();
            let sha = lines.next()?;
            lines.any(|line| line.trim_start().strip_prefix("CI-budget:").filter(|rest| rest.starts_with(char::is_whitespace)).is_some_and(|rest| speed_up(rest, workflow))).then(|| sha.to_owned())
        })
    }

    fn local_history(&self, workflow: &str, source: &str, main_red: Option<&str>, out: &mut impl Write) -> Measured {
        let path = if source.starts_with('/') { PathBuf::from(source) } else { self.tree.join(source) };
        let Ok(text) = fs::read_to_string(&path) else {
            let _ = writeln!(out, "after-landing: ✗ CI budget {workflow}: cannot read local timing source.");
            return Measured::Invalid;
        };
        let lines: Vec<&str> = text.lines().collect();
        let rows = &lines[lines.len().saturating_sub(RUNS_MEASURED)..];
        let last: Vec<&str> = rows.last().map(|row| row.split('\t').collect()).unwrap_or_default();
        if let Some(red) = main_red
            && last.first() == Some(&red)
            && !matches!(last.get(2).copied(), Some("pass" | "passed" | "success" | "ok"))
        {
            let _ = writeln!(out, "after-landing: CI budget {workflow} exemption — main is red at {red} and every commit names it in Main-red:.");
            return Measured::Exempt;
        }
        let mut history = History::default();
        for row in rows {
            let fields: Vec<&str> = row.splitn(4, '\t').collect();
            let [sha, wall, result, slowest] = [0, 1, 2, 3].map(|index| fields.get(index).copied().unwrap_or(""));
            if sha.is_empty() || matches!(result, "cancelled" | "skipped") {
                continue;
            }
            if number(wall).is_none() || result.is_empty() || slowest.is_empty() {
                let _ = writeln!(out, "after-landing: ✗ CI budget {workflow}: invalid local timing row.");
                return Measured::Invalid;
            }
            history.walls.push(wall.to_owned());
            history.newest_job = slowest.to_owned();
        }
        Measured::Runs(history)
    }

    fn remote_history(&self, workflow: &str, since: Option<&str>, main_red: Option<&str>, out: &mut impl Write) -> Measured {
        let Some(runs) = gh(self.tree, &["run", "list", "--workflow", workflow, "--branch", "main", "--status", "completed", "--limit", "10", "--json", "databaseId,attempt,conclusion,createdAt,headSha"]) else {
            return Measured::Unmeasured("gh failed to list completed main runs".to_owned());
        };
        let Some(runs) = completed_runs(&runs) else {
            return Measured::Unmeasured("cannot parse main runs".to_owned());
        };
        if let (Some(red), Some(newest)) = (main_red, runs.first())
            && newest.id == red
            && newest.conclusion == "failure"
        {
            let _ = writeln!(out, "after-landing: CI budget {workflow} exemption — main is red at run {red} and every commit names it in Main-red:.");
            return Measured::Exempt;
        }
        let mut history = History::default();
        for run in &runs {
            if let Some(since) = since
                && git::bytes(self.tree, &["merge-base", "--is-ancestor", since, run.head_sha.as_deref().unwrap_or(&format!("{since}^"))]).is_none()
            {
                continue;
            }
            let measurement = match self.measure(run) {
                Ok(Some(measurement)) => measurement,
                Ok(None) => continue,
                Err(reason) => return Measured::Unmeasured(reason),
            };
            if history.walls.is_empty() {
                history.newest_job = format!("{} ({}s)", measurement.slowest, measurement.slowest_seconds);
            }
            history.walls.push(measurement.wall.to_string());
            if history.walls.len() >= RUNS_MEASURED {
                break;
            }
        }
        Measured::Runs(history)
    }

    fn measure(&self, run: &Run) -> Result<Option<Measurement>, String> {
        let origin = git::text(self.tree, &["remote", "get-url", "origin"]).unwrap_or_default();
        let cache = self.state.join("run-measurements").join(sanitize(origin.trim_end_matches('\n'))).join(format!("{}-{}", run.id, run.attempt));
        if let Some(measurement) = fs::read_to_string(&cache).ok().and_then(|text| Measurement::parse(text.lines().next()?)) {
            return Ok(Some(measurement));
        }
        let jobs = gh(self.tree, &["api", &format!("repos/{{owner}}/{{repo}}/actions/runs/{}/jobs?per_page=100", run.id), "--paginate", "--slurp"])
            .ok_or_else(|| format!("gh failed to read jobs for run {}", run.id))?;
        let measurement = Measurement::of_jobs(&jobs).ok_or_else(|| format!("cannot measure jobs for run {}", run.id))?;
        if let Some(measurement) = &measurement
            && let Some(parent) = cache.parent()
            && fs::create_dir_all(parent).is_ok()
        {
            let _ = fs::write(&cache, format!("{}\t{}\t{}\n", measurement.wall, measurement.slowest, measurement.slowest_seconds));
        }
        Ok(measurement)
    }

    fn weigh(&self, budget: &Budget, since: Option<&str>, history: &History, out: &mut impl Write) -> Verdict {
        let workflow = budget.workflow.as_str();
        let need = if since.is_some() { 1 } else { RUNS_MEASURED };
        if history.walls.len() < need {
            let _ = writeln!(out, "after-landing: CI budget {workflow}: {} usable completed main runs (need {need}) — passing.", history.walls.len());
            return Verdict::Pass;
        }
        if let Some(since) = since {
            let _ = writeln!(out, "after-landing: CI budget {workflow}: measuring {} main runs since CI-budget commit {}.", history.walls.len(), &since[..since.len().min(12)]);
        }
        let mut walls: Vec<&String> = history.walls.iter().collect();
        walls.sort_by(|left, right| number(left).unwrap_or(0.0).total_cmp(&number(right).unwrap_or(0.0)));
        let median = walls[walls.len().div_ceil(2) - 1].as_str();
        let seconds = number(median).unwrap_or(0.0);
        if let Some(ceiling) = budget.ceiling
            && seconds > budget.seconds
            && seconds <= ceiling
        {
            self.record_debt(budget, median, out);
            self.clear(workflow);
            return Verdict::Pass;
        }
        if seconds > budget.seconds {
            let reason = format!("median {median}s > budget {}s; slowest job in most recent run: {}", budget.written.0, history.newest_job);
            return self.hold(workflow, &reason, Some((median, &budget.written.0)), out);
        }
        self.clear(workflow);
        Verdict::Pass
    }

    fn repo(&self) -> String {
        let url = git::text(self.tree, &["remote", "get-url", "origin"]).unwrap_or_default();
        let url = url.trim_end_matches('\n');
        let url = url.strip_suffix(".git").unwrap_or(url);
        url.rsplit(['/', ':']).next().unwrap_or_default().to_owned()
    }

    fn hold_file(&self, workflow: &str) -> PathBuf {
        self.state.join(format!("ci-budget-hold-{}", sanitize(&format!("{}-{workflow}", self.repo()))))
    }

    fn clear(&self, workflow: &str) {
        let _ = fs::remove_file(self.hold_file(workflow));
    }

    fn hold(&self, workflow: &str, reason: &str, measured: Option<(&str, &str)>, out: &mut impl Write) -> Verdict {
        let file = self.hold_file(workflow);
        let now = Timestamp::now().unix_seconds();
        let _ = fs::create_dir_all(&self.state);
        let first = fs::read_to_string(&file).ok().and_then(|text| text.lines().next().filter(|line| !line.is_empty() && line.bytes().all(|byte| byte.is_ascii_digit()))?.parse::<i64>().ok()).unwrap_or_else(|| {
            let _ = fs::write(&file, format!("{now}\n"));
            now
        });
        if now - first < HOLD_SECONDS {
            let _ = writeln!(out, "after-landing: ✗ CI budget {workflow}: {reason} (checked after the push; the landing stands).");
            let _ = writeln!(
                out,
                "after-landing: CI budget {workflow} has been over since {}; from {} each landing records an override instead.",
                Timestamp::from_unix_seconds(first).local_hour_minute(),
                Timestamp::from_unix_seconds(first + HOLD_SECONDS).local_hour_minute()
            );
            return Verdict::Refuse;
        }
        let (median, budget) = measured.map_or(("-".to_owned(), "-".to_owned()), |(median, budget)| (median.to_owned(), budget.to_owned()));
        let row = [Timestamp::now().iso(), self.repo(), workflow.to_owned(), median, budget, self.head_short(), self.branch(), reason.to_owned()];
        let overrides = self.state.join("ci-budget-overrides.tsv");
        append(&overrides, &row);
        let _ = writeln!(out, "after-landing: CI budget {workflow} override: {reason} for {}m; landing with a recorded override in {}.", (now - first) / 60, overrides.display());
        Verdict::Pass
    }

    fn record_debt(&self, budget: &Budget, median: &str, out: &mut impl Write) {
        let ceiling = budget.written.1.clone();
        let row = [Timestamp::now().iso(), self.repo(), budget.workflow.clone(), median.to_owned(), budget.written.0.clone(), ceiling.clone(), self.head_short(), self.branch()];
        let debt = self.state.join("ci-budget-debt.tsv");
        let _ = fs::create_dir_all(&self.state);
        append(&debt, &row);
        let _ = writeln!(
            out,
            "after-landing: CI budget {} debt: median {median}s over target {}s, under ceiling {ceiling}s; landing, recorded in {}.",
            budget.workflow,
            budget.written.0,
            debt.display()
        );
    }

    fn head_short(&self) -> String {
        git::text(self.tree, &["rev-parse", "--short", "HEAD"]).unwrap_or_default().trim_end_matches('\n').to_owned()
    }

    fn branch(&self) -> String {
        git::text(self.tree, &["branch", "--show-current"]).unwrap_or_default().trim_end_matches('\n').to_owned()
    }
}

fn append(path: &Path, row: &[String]) {
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", row.join("\t"));
    }
}

fn sanitize(text: &str) -> String {
    text.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') { character } else { '_' }).collect()
}

fn gh(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("gh").args(arguments).current_dir(directory).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

enum Measured {
    Runs(History),
    Exempt,
    Invalid,
    Unmeasured(String),
}

#[derive(Default)]
struct History {
    walls: Vec<String>,
    newest_job: String,
}

struct Run {
    id: String,
    attempt: String,
    conclusion: String,
    head_sha: Option<String>,
}

fn completed_runs(json: &str) -> Option<Vec<Run>> {
    let Value::Array(runs) = serde_json::from_str::<Value>(json).ok()? else { return None };
    let mut runs: Vec<&Value> = runs.iter().collect();
    runs.sort_by_key(|run| run.get("createdAt").and_then(Value::as_str).unwrap_or_default().to_owned());
    runs.reverse();
    let text = |run: &Value, key: &str| match run.get(key) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    };
    Some(
        runs.into_iter()
            .map(|run| Run {
                id: text(run, "databaseId").unwrap_or_else(|| "null".to_owned()),
                attempt: text(run, "attempt").unwrap_or_else(|| "1".to_owned()),
                conclusion: text(run, "conclusion").unwrap_or_default(),
                head_sha: text(run, "headSha").filter(|sha| !sha.is_empty()),
            })
            .filter(|run| run.conclusion != "cancelled" && run.conclusion != "skipped")
            .collect(),
    )
}

struct Measurement {
    wall: i64,
    slowest: String,
    slowest_seconds: i64,
}

impl Measurement {
    fn parse(line: &str) -> Option<Self> {
        let mut fields = line.split('\t');
        let wall = fields.next()?.parse().ok()?;
        let slowest = fields.next()?.to_owned();
        let slowest_seconds = fields.next()?.parse().ok()?;
        Some(Self { wall, slowest, slowest_seconds })
    }

    fn of_jobs(json: &str) -> Option<Option<Self>> {
        let Value::Array(pages) = serde_json::from_str::<Value>(json).ok()? else { return None };
        let mut jobs = Vec::new();
        for page in &pages {
            for job in page.get("jobs")?.as_array()? {
                let (Some(started), Some(completed)) = (job.get("started_at").filter(|at| !at.is_null()), job.get("completed_at").filter(|at| !at.is_null())) else {
                    continue;
                };
                let start = unix_seconds(started.as_str()?)?;
                let end = unix_seconds(completed.as_str()?)?;
                jobs.push((job.get("name").and_then(Value::as_str).unwrap_or("null").to_owned(), start, end));
            }
        }
        let start = jobs.iter().map(|job| job.1).min();
        let end = jobs.iter().map(|job| job.2).max();
        let (Some(start), Some(end)) = (start, end) else { return Some(None) };
        let slowest = jobs.iter().max_by_key(|job| job.2 - job.1)?;
        Some(Some(Self { wall: end - start, slowest: slowest.0.clone(), slowest_seconds: slowest.2 - slowest.1 }))
    }
}

fn unix_seconds(iso: &str) -> Option<i64> {
    let digits = |range: std::ops::Range<usize>| iso.get(range)?.parse::<i32>().ok();
    let shape = iso.len() == 20 && iso.as_bytes()[4] == b'-' && iso.as_bytes()[10] == b'T' && iso.ends_with('Z');
    if !shape {
        return None;
    }
    let month = time::Month::try_from(u8::try_from(digits(5..7)?).ok()?).ok()?;
    let date = time::Date::from_calendar_date(digits(0..4)?, month, u8::try_from(digits(8..10)?).ok()?).ok()?;
    let clock = time::Time::from_hms(u8::try_from(digits(11..13)?).ok()?, u8::try_from(digits(14..16)?).ok()?, u8::try_from(digits(17..19)?).ok()?).ok()?;
    Some(time::PrimitiveDateTime::new(date, clock).assume_utc().unix_timestamp())
}

#[cfg(test)]
#[path = "ci_budget_tests.rs"]
mod tests;
