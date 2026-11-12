//! The step after a DeepSeek worker reports `Done:` (north#1020). Dispatch queues one `north fleet dispatch review` job on
//! the host that holds the session, through the laptop job queue or here, as it queues launches, also for a session the
//! fleet retired before any job named it (north#1231). The job reads the lane's commit where the lane lives, over ssh on
//! the build host a session placed there ran on, and by `sampling` either launches a Sonnet reviewer on the lane and hands
//! the lane to it, or forgets the session and lands the lane through safe-push. A reviewer records its verdict with
//! `north fleet dispatch verdict`, which sampling and the DeepSeek overlay read.
use super::super::issues::{Issue, IssueRef, references};
use super::super::record::{self, Record};
use super::super::{facets, fleets};
use super::mechanical;
use super::sampling::{self, Kind, Verdict};
use super::PROVIDER;
use crate::pacing::placement;
use crate::platform::process::run_command_within;
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
const RESTING: [&str; 2] = ["idle", "parked"];
/// The classes a reviewer rejects a DeepSeek commit for, each a mistake the lander saw from DeepSeek workers on 2026-11-09/10.
pub const CLASSES: [(&str, &str); 9] = [
    ("test-passes-on-base", "an added test passes with the lane's non-test changes reverted"),
    ("test-budget", "the lane grows the test count past the verification budget of 20 s without deleting as many tests"),
    ("restored-revert", "it restores work that a revert on main removed"),
    ("test-reads-host", "a test reads /proc, the wall clock or another live host fact"),
    ("unchecked-tick", "it ticks a box without running that box's check"),
    ("duplicate", "origin/main already holds a commit of the same name"),
    ("oracle-edit", "it edits the file defining a Done-when test that stood before staffing"),
    ("out-of-scope", "it changes behaviour the box does not ask for"),
    ("other-defect", "any other real defect"),
];

/// Who lands a DeepSeek worker's commit. Never the worker: its lane-only shell refuses safe-push (north#1041).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lander {
    /// A Sonnet reviewer the review job launches on the lane, for a commit `sampling` picks.
    Reviewer,
    /// The review job itself, through safe-push, for a commit `sampling` passes over.
    ReviewJob,
}

impl Lander {
    /// The landing of `sha`, a commit of a box in `category`, by the verdicts so far.
    pub fn of(verdicts: &[Verdict], category: &str, landing: bool, locked: bool, sha: &str) -> Self {
        if sampling::reviews(verdicts, category, landing, locked, sha) { Self::Reviewer } else { Self::ReviewJob }
    }

    fn says(self) -> &'static str {
        match self {
            Self::Reviewer => "a commit sampling picks goes to a Sonnet reviewer, who lands it through safe-push",
            Self::ReviewJob => "dispatch's review job lands any other commit through safe-push itself",
        }
    }
}

/// The order every DeepSeek worker's brief carries, naming each `Lander` the review job may take.
pub fn worker_order() -> String {
    format!(
        "Lane-only: never run safe-push; it refuses at once with `lane-only session: end with Done: <worktree> <branch> <sha>; the review-and-land loop lands it`. \
         Commit in your lane with this brief's `Item:` and `Box:` lines in the commit message, and report Done: with that line. Then {}, and {}; \
         the lander ticks the box from those lines once the commit is on main.",
        Lander::Reviewer.says(),
        Lander::ReviewJob.says()
    )
}

/// A DeepSeek session that reported Done and waits for its review job.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub name: String,
    pub host: String,
    pub item: IssueRef,
    pub box_text: Option<String>,
    pub category: String,
    pub landing: bool,
    /// The Done-when tests of the box, named by their `test(/NAME/)` filters.
    pub oracle: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Queued {
    at: f64,
    name: String,
}

/// The sessions the review record names: within `REQUEUE_S`, so one Done report queues one job, and ever.
#[derive(Clone, Debug, Default)]
pub struct Marked {
    pub recent: BTreeSet<String>,
    pub ever: BTreeSet<String>,
}

/// The sessions dispatch queued a review job for, as of `at`.
pub fn queued(directory: &Path, at: f64) -> Marked {
    let text = std::fs::read_to_string(directory.join(QUEUED)).unwrap_or_default();
    let marks: Vec<Queued> = text.lines().filter_map(|line| serde_json::from_str::<Queued>(line).ok()).collect();
    Marked { recent: marks.iter().filter(|queued| at - queued.at < REQUEUE_S).map(|queued| queued.name.clone()).collect(), ever: marks.into_iter().map(|queued| queued.name).collect() }
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

/// Whether `session` is the provider's and waits for its review job: resting with none queued lately, or retired by its
/// host before any job named it.
fn waits(session: &Value, marked: &Marked) -> bool {
    let name = session["name"].as_str().unwrap_or_default();
    session["provider"] == PROVIDER
        && if session["past"] == true { session["ended"] == "retired" && !marked.ever.contains(name) } else { RESTING.contains(&session["status"].as_str().unwrap_or_default()) && !marked.recent.contains(name) }
}

/// Whether `listing` holds a session of the provider that waits for its review job.
pub fn has_waiting(listing: &Value, marked: &Marked) -> bool {
    listing.as_array().into_iter().flatten().any(|session| waits(session, marked))
}

/// The DeepSeek sessions in each host's listing that wait for their review job and whose last report starts `Done:`; a
/// retired one only while its issue is open.
pub fn done(lists: &[(String, Value)], reports: &[(String, Value)], records: &[Record], issues: &[Issue], marked: &Marked) -> Vec<Done> {
    let mut found = Vec::new();
    for (host, listing) in lists {
        let Some(report) = reports.iter().find(|(name, _)| name == host).map(|(_, report)| report) else { continue };
        for session in listing.as_array().into_iter().flatten().filter(|session| waits(session, marked)) {
            let name = session["name"].as_str().unwrap_or_default();
            if !reported_done(&report[name]) {
                continue;
            }
            let tasks = session["tasks"].as_array().map_or_else(|| session["task_ref"].as_str().unwrap_or_default().to_owned(), |tasks| tasks.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "));
            let Some(item) = references(&tasks, "").into_iter().find(|item| !item.repo.is_empty()).or_else(|| fleets::named_items(name).into_iter().next()) else { continue };
            let issue = issues.iter().find(|issue| issue.reference() == item);
            if issue.is_none() && session["past"] == true {
                continue;
            }
            let category = records.iter().rev().find(|record| record.name == name).and_then(|record| record.category.clone()).or_else(|| issue.and_then(|issue| facets::declared_in(&issue.body, "Category")).map(str::to_owned)).unwrap_or_else(|| "unknown".into());
            let landing = issue.is_some_and(|issue| issue.labels.iter().any(|label| label == LANDING_LABEL));
            found.push(Done { name: name.to_owned(), host: host.clone(), item, box_text: session["box"].as_str().map(str::to_owned), category, landing, oracle: issue.map(|issue| oracle_tests(&issue.body, session["box"].as_str())).unwrap_or_default() });
        }
    }
    found
}

/// The test names the box's own line of `body` gives as `test(/NAME/)` filters.
fn oracle_tests(body: &str, box_text: Option<&str>) -> Vec<String> {
    let line = box_text.and_then(|text| body.lines().find(|line| line.contains(text))).unwrap_or_default();
    line.split("test(/").skip(1).filter_map(|rest| rest.split_once('/').map(|(name, _)| name.to_owned())).collect()
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
    for test in &done.oracle {
        words.extend(["--oracle".to_owned(), test.clone()]);
    }
    words
}

/// Runs `north WORDS` on a host other than this one and the laptop.
pub type Remote<'a> = dyn Fn(&str, &[String]) -> Result<(), String> + 'a;

/// Queues the review job of each session in `dones`: on the laptop through its job queue, here by running it, on any other host by running it there over ssh; prints one line each.
pub fn queue(dones: &[Done], here: &str, laptop: Option<&str>, remote: &Remote<'_>, dry_run: bool) -> Vec<String> {
    let north = std::env::current_exe().map_or_else(|_| "north".into(), |path| path.to_string_lossy().into_owned());
    let directory = record::directory();
    dones
        .iter()
        .map(|done| {
            let line = format!("review {} on {} ({}, {})", done.name, done.host, done.item, done.category);
            if dry_run {
                return format!("would {line}");
            }
            let ran = if laptop == Some(done.host.as_str()) {
                let mut queued = ["threads", "queue", "add", "laptop", &done.item.to_string(), "--", "north"].map(str::to_owned).to_vec();
                queued.extend(job(done));
                run(&north, &queued)
            } else if done.host == here {
                run(&north, &job(done))
            } else {
                remote(&done.host, &job(done))
            };
            match ran.and_then(|()| mark(&directory, &done.name, placement::now())) {
                Ok(()) => line,
                Err(why) => format!("{line} failed: {why}"),
            }
        })
        .collect()
}

fn run(north: &str, words: &[String]) -> Result<(), String> {
    let output = Command::new(north).args(words).output().map_err(|error| error.to_string())?;
    if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}

#[derive(Debug, PartialEq)]
struct Job {
    name: String,
    item: String,
    category: String,
    box_text: Option<String>,
    landing: bool,
    oracle: Vec<String>,
}

fn parse(arguments: &[String]) -> Option<Job> {
    let (name, mut rest) = arguments.split_first()?;
    let mut job = Job { name: name.clone(), item: String::new(), category: String::new(), box_text: None, landing: false, oracle: Vec::new() };
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
            "--oracle" => job.oracle.push(value.clone()),
            _ => return None,
        }
    }
    (!job.item.is_empty() && !job.category.is_empty()).then_some(job)
}

/// A lane and the machine it lives on: this host, or the build host a session placed there ran on, reached over ssh.
struct Worktree {
    path: PathBuf,
    machine: Option<(String, String)>,
}

impl Worktree {
    fn here(path: PathBuf) -> Self {
        Self { path, machine: None }
    }

    /// The command that runs `words` in this lane's directory where it lives.
    fn command(&self, words: &[&str]) -> Result<Command, String> {
        let path = self.path.to_str().ok_or("the lane path is not UTF-8")?;
        let (program, rest) = words.split_first().ok_or("no command")?;
        let Some((_, ssh)) = &self.machine else {
            let mut command = Command::new(crate::fleet::runtime::binary(program));
            command.args(rest).current_dir(&self.path);
            return Ok(command);
        };
        let script = format!("cd {} && PATH=\"$HOME/.local/bin:$PATH\" {}", quote(path), words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
        let mut command = Command::new(crate::fleet::runtime::binary("ssh"));
        command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", ssh, "--", &script]);
        Ok(command)
    }

    fn exists(&self) -> bool {
        match &self.machine {
            None => self.path.is_dir(),
            Some(_) => self.command(&["true"]).ok().and_then(|command| run_command_within(command, None, GIT_WAIT)).is_some_and(|output| output.status.success()),
        }
    }

    /// The `--cwd` a session launched from this host takes to work in this lane.
    fn launch_cwd(&self) -> String {
        match &self.machine {
            Some((name, _)) => format!("{name}:{}", self.path.display()),
            None => self.path.to_string_lossy().into_owned(),
        }
    }
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

fn git(lane: &Worktree, args: &[&str]) -> Result<String, String> {
    let mut words = vec!["git"];
    words.extend(args);
    let output = run_command_within(lane.command(&words)?, None, GIT_WAIT).ok_or_else(|| format!("git {} did not finish", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The lane a DeepSeek session worked in: `worktrees/NAME` beside the `main` checkout it was launched in, which the
/// opencode launcher makes on the machine the session ran on, else that checkout when it ran here.
fn lane(session: &Value, name: &str) -> Worktree {
    let cwd = Path::new(session["cwd"].as_str().unwrap_or_default());
    let made = cwd.parent().map(|container| container.join("worktrees").join(name));
    match session["machine"].as_str() {
        Some(machine) => {
            let ssh = placement::config().map_or_else(|_| machine.to_owned(), |cfg| fleets::destination(&cfg, machine));
            Worktree { path: made.unwrap_or_else(|| cwd.to_owned()), machine: Some((machine.to_owned(), ssh)) }
        }
        None => Worktree::here(made.filter(|lane| lane.is_dir()).unwrap_or_else(|| cwd.to_owned())),
    }
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
         If it cannot land, run `{verdict} defect CLASS`, which records the rejection on the lane so `north lane gc` bundles it to ~/archive/lanes with your verdict and removes it once you exit, and report Not done naming the class; dispatch then gives the box to Claude.\n",
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

/// `Landed` when the lander landed a ticket of `session`'s lane before its Done report, else `Empty`.
pub(super) fn no_commit_kind(queues: &Path, session: &str) -> Kind {
    let landed = north_base::land_ticket::lanes_of(queues, session).is_ok_and(|lanes| lanes.iter().any(|lane| lane.phase == north_base::land_ticket::Phase::Landed));
    if landed { Kind::Landed } else { Kind::Empty }
}

fn review(job: &Job) -> Result<String, String> {
    let listing: Value = serde_json::from_str(&north(&["fleet", "list", "--json", "--all"])?).map_err(|error| error.to_string())?;
    let Some(session) = listing.as_array().into_iter().flatten().filter(|session| session["name"] == job.name.as_str()).min_by_key(|session| session["past"] == true) else { return Ok(format!("{} is no longer listed", job.name)) };
    let retired = session["past"] == true;
    let lane = lane(session, &job.name);
    if retired && !lane.exists() {
        return Ok(format!("{} was retired and its lane {} is gone", job.name, lane.path.display()));
    }
    let forget = |to: Option<&str>| -> Result<(), String> {
        if retired {
            return Ok(());
        }
        let mut words = vec!["fleet", "forget", job.name.as_str()];
        words.extend(to.map(|heir| ["--to", heir]).into_iter().flatten());
        north(&words).map(drop)
    };
    git(&lane, &["fetch", "-q", "origin"])?;
    let sha = git(&lane, &["rev-parse", "HEAD"])?;
    let ahead: u32 = git(&lane, &["rev-list", "--count", "origin/main..HEAD"])?.parse().map_err(|error| format!("{error}"))?;
    let directory = record::directory();
    let verdict = |kind: Kind, class: Option<String>| Verdict { at: placement::now(), session: job.name.clone(), category: job.category.clone(), sha: sha.clone(), kind, class, effort: None };
    if ahead == 0 {
        forget(None)?;
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|path| path.is_absolute()).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        let kind = no_commit_kind(&state.join("north/land-queue"), &job.name);
        sampling::append(&directory, &verdict(kind, None))?;
        return Ok(format!("{} reported Done with no commit ahead of origin/main ({kind:?}); forgot it", job.name));
    }
    if let Some(failed) = mechanical_failure(&lane, &job.oracle)? {
        if !retired {
            let told = format!("A mechanical check refused your commit {sha} before any review: {} ({}). Fix it in the lane and report Done again.", failed.why, failed.class);
            north(&["fleet", "send", &job.name, &told])?;
        }
        sampling::append(&directory, &verdict(Kind::Bounced, Some(failed.class.to_owned())))?;
        return Ok(format!("{} bounced to {}: {}; no reviewer launched", job.item, job.name, failed.class));
    }
    let locked = !oracle_files(&lane, &git(&lane, &["merge-base", "origin/main", "HEAD"])?, &job.oracle).is_empty();
    if Lander::of(&sampling::read(&directory), &job.category, job.landing, locked, &sha) == Lander::Reviewer {
        let reviewer = launch_reviewer(job, &lane, &sha)?;
        forget(Some(&reviewer))?;
        return Ok(format!("{reviewer} reviews {} ({sha}, {}); {} forgotten", job.item, job.category, job.name));
    }
    forget(None)?;
    let pushed = lane.command(&["safe-push"])?.output().map_err(|error| error.to_string())?;
    if pushed.status.success() {
        sampling::append(&directory, &verdict(Kind::Unsampled, None))?;
        return Ok(format!("{} ({sha}) went to the lander unreviewed; {} forgotten", job.item, job.name));
    }
    let refusal = String::from_utf8_lossy(&pushed.stderr).lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("safe-push refused").to_owned();
    sampling::append(&directory, &verdict(Kind::Refused, Some(refusal.clone())))?;
    Err(format!("safe-push refused {sha}: {refusal}; the box goes to Claude"))
}

/// Launches the Sonnet reviewer of `job`'s commit `sha` where `lane` lives, and names it.
fn launch_reviewer(job: &Job, lane: &Worktree, sha: &str) -> Result<String, String> {
    let reviewer = format!("{}-rv", job.name);
    let effort = sampling::reviewer_effort(&job.name);
    let text = brief(&job.name, &job.item, &job.category, job.box_text.as_deref(), &lane.path, sha, effort);
    let cwd = lane.launch_cwd();
    let mut words = vec!["fleet", "launch", "--provider", "claude", "--model", "sonnet", "--effort", effort, "--role", "worker", "--name", &reviewer, "--task", &job.item, "--cwd", &cwd];
    words.extend(job.box_text.as_deref().map(|text| ["--box", text]).into_iter().flatten());
    words.extend(["--tier-reason", "north#1020: Sonnet reviews DeepSeek commits by sampling"]);
    if lane.machine.is_none() {
        words.extend(["--local", "the DeepSeek lane is on this host"]);
    }
    words.push(&text);
    north(&words)?;
    Ok(reviewer)
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
    let rejected = class.clone().filter(|_| kind == Kind::Defect);
    match sampling::append(&record::directory(), &Verdict { at: placement::now(), session: session.clone(), category: category.clone(), sha: sha.clone(), kind, class, effort }) {
        Ok(()) => {
            if let Some(class) = rejected {
                reject_lane(session, sha, &class);
            }
            0
        }
        Err(why) => {
            eprintln!("north fleet dispatch verdict: {why}");
            1
        }
    }
}

fn reject_lane(session: &str, sha: &str, class: &str) {
    let Ok(top) = std::env::current_dir().map_err(|error| error.to_string()).and_then(|cwd| git(&Worktree::here(cwd), &["rev-parse", "--show-toplevel"])) else { return };
    if !Path::new(&top).parent().is_some_and(|parent| parent.ends_with("worktrees")) {
        return;
    }
    match north(&["lane", "remove", "--rejected", &format!("{class} (review of {session} at {sha})"), &top]) {
        Ok(line) => println!("{line}"),
        Err(why) => eprintln!("north fleet dispatch verdict: the rejected lane {top} stays: {why}"),
    }
}

/// The files at `base` that define the tests named `oracle`.
fn oracle_files(lane: &Worktree, base: &str, oracle: &[String]) -> Vec<String> {
    oracle.iter().flat_map(|name| git(lane, &["grep", "-l", "-e", &format!("fn {name}"), base, "--", "*.rs"]).unwrap_or_default().lines().filter_map(|hit| hit.split_once(':').map(|(_, file)| file.to_owned())).collect::<Vec<_>>()).collect()
}

/// The first mechanical check the lane's head fails, run before any reviewer launches.
fn mechanical_failure(lane: &Worktree, oracle: &[String]) -> Result<Option<mechanical::Failure>, String> {
    let base = git(lane, &["merge-base", "origin/main", "HEAD"])?;
    let diff = git(lane, &["diff", "-U0", &format!("{base}..HEAD")])?;
    let subject = git(lane, &["log", "-1", "--format=%s"])?;
    let main_subjects: Vec<String> = git(lane, &["log", "--format=%s", "-n", "2000", "origin/main"])?.lines().map(str::to_owned).collect();
    let oracle_files = oracle_files(lane, &base, oracle);
    Ok(mechanical::failing(&mechanical::Lane { diff: &diff, subject: &subject, main_subjects: &main_subjects, oracle_files: &oracle_files }))
}
