//! `north fleet dispatch review NAME ...`: one review job on the lane a session left, which launches a reviewer or lands
//! the lane itself.

use super::brief::{brief, Lander};
use super::lane::{git, lane, mechanical_failure, north, oracle_files, Worktree};
use super::super::sampling::{self, Kind, Verdict};
use super::super::super::record;
use crate::pacing::placement;
use serde_json::Value;
use std::path::{Path, PathBuf};

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

/// `north fleet dispatch review NAME --item REPO#N --category C [--box TEXT] [--landing]`, the job dispatch queues on the
/// host of a DeepSeek session that reported Done.
pub fn serve(arguments: &[String]) -> u8 {
    let Some(job) = parse(arguments) else {
        eprintln!("usage: north fleet dispatch review NAME --item REPO#N --category C [--box TEXT] [--landing]");
        return 2;
    };
    if let Some(reason) = super::super::super::pause::standing() {
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
pub(crate) fn no_commit_kind(queues: &Path, session: &str) -> Kind {
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
