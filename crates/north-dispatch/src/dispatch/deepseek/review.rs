//! The step after a DeepSeek worker reports `Done:` (north#1020). Dispatch queues one `north fleet dispatch review` job on
//! the session's host, through the laptop job queue or here, as it queues launches. The job reads the lane's commit, and
//! by `sampling` either launches a Sonnet reviewer on the lane and hands the lane to it, or forgets the session and lands
//! the lane through safe-push. A reviewer records its verdict with `north fleet dispatch verdict`, which sampling and
//! the DeepSeek overlay read.
use super::super::issues::{Issue, IssueRef, references};
use super::super::record::{self, Record};
use super::super::{facets, fleets};
use super::sampling::{self, Kind, Verdict};
use super::PROVIDER;
use crate::pacing::placement;
use crate::platform::process::run_within;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const QUEUED: &str = "deepseek-reviews.jsonl";
const REQUEUE_S: f64 = 30.0 * 60.0;
const GIT_WAIT: Duration = Duration::from_secs(60);
const LANDING_LABEL: &str = "area:landing";
/// The classes a reviewer rejects a DeepSeek commit for, each a mistake the lander saw from DeepSeek workers on 2026-11-09/10.
pub const CLASSES: [(&str, &str); 7] = [
    ("test-passes-on-base", "an added test passes with the lane's non-test changes reverted"),
    ("test-budget", "the lane grows the test count past the verification budget of 20 s without deleting as many tests"),
    ("restored-revert", "it restores work that a revert on main removed"),
    ("test-reads-host", "a test reads /proc, the wall clock or another live host fact"),
    ("unchecked-tick", "it ticks a box without running that box's check"),
    ("out-of-scope", "it changes behaviour the box does not ask for"),
    ("other-defect", "any other real defect"),
];

/// A DeepSeek session that reported Done and waits for its review job.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub name: String,
    pub host: String,
    pub item: IssueRef,
    pub box_text: Option<String>,
    pub category: String,
    pub landing: bool,
}

#[derive(Deserialize, Serialize)]
struct Queued {
    at: f64,
    name: String,
}

/// The sessions dispatch queued a review job for within `REQUEUE_S`, so one Done report queues one job.
pub fn queued(directory: &Path, at: f64) -> BTreeSet<String> {
    let text = std::fs::read_to_string(directory.join(QUEUED)).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str::<Queued>(line).ok()).filter(|queued| at - queued.at < REQUEUE_S).map(|queued| queued.name).collect()
}

fn mark(directory: &Path, name: &str, at: f64) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(QUEUED)).map_err(|error| error.to_string())?;
    let line = format!("{}\n", serde_json::to_string(&Queued { at, name: name.to_owned() }).map_err(|error| error.to_string())?);
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())
}

fn reported_done(report: &Value) -> bool {
    report["entries"].as_array().and_then(|entries| entries.iter().rev().find(|entry| entry["kind"] == "assistant")).and_then(|entry| entry["text"].as_str()).is_some_and(|text| text.trim_start().starts_with("Done:"))
}

/// The idle DeepSeek sessions in each host's listing whose last report starts `Done:`, with no review job queued.
pub fn done(lists: &[(String, Value)], reports: &[(String, Value)], records: &[Record], issues: &[Issue], queued: &BTreeSet<String>) -> Vec<Done> {
    let mut found = Vec::new();
    for (host, listing) in lists {
        let Some(report) = reports.iter().find(|(name, _)| name == host).map(|(_, report)| report) else { continue };
        for session in listing.as_array().into_iter().flatten().filter(|session| session["provider"] == PROVIDER && session["status"] == "idle") {
            let name = session["name"].as_str().unwrap_or_default();
            if queued.contains(name) || !reported_done(&report[name]) {
                continue;
            }
            let tasks = session["tasks"].as_array().map_or_else(|| session["task_ref"].as_str().unwrap_or_default().to_owned(), |tasks| tasks.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "));
            let Some(item) = references(&tasks, "").into_iter().find(|item| !item.repo.is_empty()).or_else(|| fleets::named_items(name).into_iter().next()) else { continue };
            let issue = issues.iter().find(|issue| issue.reference() == item);
            let category = records.iter().rev().find(|record| record.name == name).and_then(|record| record.category.clone()).or_else(|| issue.and_then(|issue| facets::declared_in(&issue.body, "Category")).map(str::to_owned)).unwrap_or_else(|| "unknown".into());
            let landing = issue.is_some_and(|issue| issue.labels.iter().any(|label| label == LANDING_LABEL));
            found.push(Done { name: name.to_owned(), host: host.clone(), item, box_text: session["box"].as_str().map(str::to_owned), category, landing });
        }
    }
    found
}

/// The `north` words of the review job for `done`.
pub fn job(done: &Done) -> Vec<String> {
    let mut words: Vec<String> = ["fleet", "dispatch", "review", &done.name, "--item", &done.item.to_string(), "--category", &done.category].map(str::to_owned).into();
    if let Some(text) = &done.box_text {
        words.extend(["--box".to_owned(), text.clone()]);
    }
    if done.landing {
        words.push("--landing".into());
    }
    words
}

/// Queues the review job of each session in `dones`: on the laptop through its job queue, here by running it; prints one line each.
pub fn queue(dones: &[Done], here: &str, laptop: Option<&str>, dry_run: bool) -> Vec<String> {
    let north = std::env::current_exe().map_or_else(|_| "north".into(), |path| path.to_string_lossy().into_owned());
    let directory = record::directory();
    dones
        .iter()
        .map(|done| {
            let line = format!("review {} on {} ({}, {})", done.name, done.host, done.item, done.category);
            if dry_run {
                return format!("would {line}");
            }
            let words = if laptop == Some(done.host.as_str()) {
                let mut queued = ["threads", "queue", "add", "laptop", &done.item.to_string(), "--", "north"].map(str::to_owned).to_vec();
                queued.extend(job(done));
                queued
            } else if done.host == here {
                job(done)
            } else {
                return format!("{line}: skipped, dispatch reaches DeepSeek sessions only here and on the laptop");
            };
            let ran = Command::new(&north).args(&words).output().map_err(|error| error.to_string()).and_then(|output| if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) });
            match ran.and_then(|()| mark(&directory, &done.name, placement::now())) {
                Ok(()) => line,
                Err(why) => format!("{line} failed: {why}"),
            }
        })
        .collect()
}

#[derive(Debug, PartialEq)]
struct Job {
    name: String,
    item: String,
    category: String,
    box_text: Option<String>,
    landing: bool,
}

fn parse(arguments: &[String]) -> Option<Job> {
    let (name, mut rest) = arguments.split_first()?;
    let mut job = Job { name: name.clone(), item: String::new(), category: String::new(), box_text: None, landing: false };
    while let Some((flag, tail)) = rest.split_first() {
        rest = tail;
        if flag == "--landing" {
            job.landing = true;
            continue;
        }
        let (value, tail) = rest.split_first()?;
        rest = tail;
        match flag.as_str() {
            "--item" => job.item = value.clone(),
            "--category" => job.category = value.clone(),
            "--box" => job.box_text = Some(value.clone()),
            _ => return None,
        }
    }
    (!job.item.is_empty() && !job.category.is_empty()).then_some(job)
}

fn git(lane: &Path, args: &[&str]) -> Result<String, String> {
    let mut words = vec!["-C", lane.to_str().ok_or("the lane path is not UTF-8")?];
    words.extend(args);
    let output = run_within("git", &words, GIT_WAIT).ok_or_else(|| format!("git {} did not finish", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The lane a DeepSeek session worked in: `worktrees/NAME` beside the `main` checkout it was launched in, which the
/// opencode launcher makes, else that checkout.
fn lane(cwd: &Path, name: &str) -> PathBuf {
    let made = cwd.parent().map(|container| container.join("worktrees").join(name));
    made.filter(|lane| lane.is_dir()).unwrap_or_else(|| cwd.to_owned())
}

fn north(words: &[&str]) -> Result<String, String> {
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let output = Command::new(program).args(words).output().map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() { Ok(text) } else { Err(format!("north {}: {} {}", words.first().copied().unwrap_or_default(), text, String::from_utf8_lossy(&output.stderr).trim())) }
}

/// The brief of the Sonnet reviewer of `job`'s commit `sha` in `lane`.
pub fn brief(job_name: &str, item: &str, category: &str, box_text: Option<&str>, lane: &Path, sha: &str, effort: &str) -> String {
    let classes: String = CLASSES.iter().map(|(class, what)| format!("- `{class}`: {what}\n")).collect();
    let verdict = format!("north fleet dispatch verdict --session {job_name} --category {category} --sha {sha} --effort {effort}");
    format!(
        "Category: review\nItem: {item}\n{}Review and land the DeepSeek worker {job_name}'s commits in {} (head {sha}) against the box above.\n\n\
         Read `git -C {} log -p origin/main..HEAD` and the box's Done when, and run that check in the lane. Reject the commit for any of these classes:\n{classes}\n\
         If it is clean, land it with `safe-push` from the lane and run `{verdict} clean`. If you fix a real defect yourself, land the fixed lane and run `{verdict} defect CLASS`. \
         If it cannot land, run `{verdict} defect CLASS` and report Not done naming the class; dispatch then gives the box to Claude.\n",
        box_text.map_or_else(String::new, |text| format!("Box: {text}\n")),
        lane.display(),
        lane.display(),
    )
}

/// `north fleet dispatch review NAME --item REPO#N --category C [--box TEXT] [--landing]`, the job dispatch queues on the
/// host of a DeepSeek session that reported Done.
pub fn serve(arguments: &[String]) -> u8 {
    let Some(job) = parse(arguments) else {
        eprintln!("usage: north fleet dispatch review NAME --item REPO#N --category C [--box TEXT] [--landing]");
        return 2;
    };
    if let Some(reason) = super::super::pause::standing() {
        eprintln!("north fleet dispatch review {}: dispatch is paused here, so the session waits for the next run: {reason}", job.name);
        return 1;
    }
    match review(&job) {
        Ok(line) => {
            println!("north fleet dispatch review: {line}");
            0
        }
        Err(why) => {
            eprintln!("north fleet dispatch review {}: {why}", job.name);
            1
        }
    }
}

fn review(job: &Job) -> Result<String, String> {
    let listing: Value = serde_json::from_str(&north(&["fleet", "list", "--json"])?).map_err(|error| error.to_string())?;
    let Some(session) = listing.as_array().into_iter().flatten().find(|session| session["name"] == job.name.as_str()) else { return Ok(format!("{} is no longer listed", job.name)) };
    let lane = lane(Path::new(session["cwd"].as_str().unwrap_or_default()), &job.name);
    git(&lane, &["fetch", "-q", "origin"])?;
    let sha = git(&lane, &["rev-parse", "HEAD"])?;
    let ahead: u32 = git(&lane, &["rev-list", "--count", "origin/main..HEAD"])?.parse().map_err(|error| format!("{error}"))?;
    let directory = record::directory();
    let verdict = |kind: Kind, class: Option<String>| Verdict { at: placement::now(), session: job.name.clone(), category: job.category.clone(), sha: sha.clone(), kind, class, effort: None };
    if ahead == 0 {
        north(&["fleet", "forget", &job.name])?;
        sampling::append(&directory, &verdict(Kind::Empty, None))?;
        return Ok(format!("{} reported Done with no commit ahead of origin/main; forgot it", job.name));
    }
    if sampling::reviews(&sampling::read(&directory), &job.category, job.landing, &sha) {
        let reviewer = format!("{}-rv", job.name);
        let effort = sampling::reviewer_effort(&job.name);
        let text = brief(&job.name, &job.item, &job.category, job.box_text.as_deref(), &lane, &sha, effort);
        let cwd = lane.to_string_lossy().into_owned();
        let mut words = vec!["fleet", "launch", "--provider", "claude", "--model", "sonnet", "--effort", effort, "--role", "worker", "--name", &reviewer, "--task", &job.item, "--cwd", &cwd];
        words.extend(job.box_text.as_deref().map(|text| ["--box", text]).into_iter().flatten());
        words.extend(["--tier-reason", "north#1020: Sonnet reviews DeepSeek commits by sampling", "--local", "the DeepSeek lane is on this host", &text]);
        north(&words)?;
        north(&["fleet", "forget", &job.name, "--to", &reviewer])?;
        return Ok(format!("{reviewer} reviews {} ({sha}, {}); {} forgotten", job.item, job.category, job.name));
    }
    north(&["fleet", "forget", &job.name])?;
    let pushed = Command::new(crate::fleet::runtime::binary("safe-push")).current_dir(&lane).output().map_err(|error| error.to_string())?;
    if pushed.status.success() {
        sampling::append(&directory, &verdict(Kind::Unsampled, None))?;
        return Ok(format!("{} ({sha}) went to the lander unreviewed; {} forgotten", job.item, job.name));
    }
    let refusal = String::from_utf8_lossy(&pushed.stderr).lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("safe-push refused").to_owned();
    sampling::append(&directory, &verdict(Kind::Refused, Some(refusal.clone())))?;
    Err(format!("safe-push refused {sha}: {refusal}; the box goes to Claude"))
}

/// `north fleet dispatch verdict --session NAME --category C --sha SHA [--effort medium|high] clean | defect CLASS`: a reviewer's verdict on a
/// DeepSeek commit.
pub fn verdict(arguments: &[String]) -> u8 {
    let usage = || {
        eprintln!("usage: north fleet dispatch verdict --session NAME --category C --sha SHA [--effort medium|high] clean | defect CLASS ({})", CLASSES.map(|(class, _)| class).join(", "));
        2
    };
    let [session_flag, session, category_flag, category, sha_flag, sha, rest @ ..] = arguments else { return usage() };
    if (session_flag.as_str(), category_flag.as_str(), sha_flag.as_str()) != ("--session", "--category", "--sha") {
        return usage();
    }
    let (effort, rest) = match rest {
        [flag, effort, rest @ ..] if flag == "--effort" && sampling::REVIEWER_EFFORTS.contains(&effort.as_str()) => (Some(effort.clone()), rest),
        _ => (None, rest),
    };
    let (kind, class) = match rest {
        [clean] if clean == "clean" => (Kind::Clean, None),
        [defect, class] if defect == "defect" && CLASSES.iter().any(|(known, _)| known == class) => (Kind::Defect, Some(class.clone())),
        _ => return usage(),
    };
    match sampling::append(&record::directory(), &Verdict { at: placement::now(), session: session.clone(), category: category.clone(), sha: sha.clone(), kind, class, effort }) {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("north fleet dispatch verdict: {why}");
            1
        }
    }
}
