//! Staffing's check that a launch does not start on files lanes already in flight have changed: it
//! names each overlapping lane and file, and queues the box behind the earliest lane when two or more
//! in-flight lanes already changed one of its files, unless `--overlap-ok REASON` starts it anyway.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::LaunchRequest;
use crate::fleet::overlap::{Lane, QUEUED, MergeDriven, Policy, Verdict, assess, declared_files, follows_line, warnings};
use crate::fleet::registry::Fleet;
use crate::pacing::placement::config;
use crate::platform::follows::follows_line as brief_follows;
use crate::threads::catalog::CATALOG;

use super::placement;

pub(super) enum Outcome {
    Start,
    Queued(String),
}

pub(super) fn guard(fleet: &Fleet, request: &LaunchRequest, tier: &str, excuse: Option<&str>) -> NorthResult<Outcome> {
    let mut declared = declared_files(&request.prompt);
    if declared.is_empty() {
        declared = declared_files(&placement::brief_text(request.system_prompt.as_deref(), "")?);
    }
    let Some(repo) = Repo::of(&request.cwd).filter(|_| !declared.is_empty() && !request.verification) else { return Ok(Outcome::Start) };
    let lanes = others(in_flight(fleet, &repo, &request.name), &request.tasks());
    let driven = repo.attributes().map(|text| MergeDriven::parse(&text)).unwrap_or_default();
    let verdict = assess(&declared, &lanes, &|file| driven.covers(file), policy());
    for line in warnings(&verdict) {
        eprintln!("{line}");
    }
    if let Some(reason) = excuse.filter(|_| !matches!(verdict, Verdict::Clear)) {
        eprintln!("overlap-ok: {} starts beside the lanes above: {reason}", request.name);
        return Ok(Outcome::Start);
    }
    let Verdict::Queue { behind, hot, .. } = &verdict else { return Ok(Outcome::Start) };
    let (Some(item), Some(parent), Some(follows)) = (request.task(), request.parent.as_deref(), follows_line(behind, hot)) else {
        eprintln!("overlap: {} starts anyway: it has no task or no parent to queue it behind lane {}", request.name, behind.lane);
        return Ok(Outcome::Start);
    };
    if brief_follows(&request.prompt).is_some() {
        eprintln!("overlap: {} already waited on its Follows line, so it starts", request.name);
        return Ok(Outcome::Start);
    }
    let job = queue(&item, tier, parent, &format!("{}\n\n{follows}\n", request.prompt.trim_end()))?;
    Ok(Outcome::Queued(format!("{QUEUED} {item} waits as follows box {job} behind lane {} ({}); rerun with --overlap-ok REASON to start it now", behind.lane, hot.join(", "))))
}

pub(super) fn noted(placement: Option<String>, excuse: Option<&str>) -> Option<String> {
    match (placement, excuse) {
        (note, None) => note,
        (None, Some(reason)) => Some(format!("overlap-ok: {reason}")),
        (Some(note), Some(reason)) => Some(format!("{note}; overlap-ok: {reason}")),
    }
}

fn policy() -> Policy {
    let defaults = Policy::default();
    let Ok(cfg) = config() else { return defaults };
    let staffing = cfg.get("staffing");
    Policy {
        follows_on_overlap: staffing.and_then(|table| table.get("follows-on-overlap")).and_then(toml_edit::Item::as_bool).unwrap_or(defaults.follows_on_overlap),
        hot_lanes: staffing.and_then(|table| table.get("overlap-lanes")).and_then(toml_edit::Item::as_integer).and_then(|lanes| usize::try_from(lanes).ok()).filter(|lanes| *lanes > 0).unwrap_or(defaults.hot_lanes),
    }
}

/// The lanes that are not the launch's own: a box relaunched for an item whose lane is in flight continues that lane, not collides with it.
fn others(mut lanes: Vec<Lane>, own: &[String]) -> Vec<Lane> {
    let own: Vec<String> = own.iter().map(|task| canonical(task)).collect();
    lanes.retain(|lane| lane.task.as_deref().is_none_or(|task| !own.contains(&canonical(task))));
    lanes
}

fn canonical(item: &str) -> String {
    CATALOG.parse_ref(item).map_or_else(|_| item.to_owned(), |(repo, number)| format!("{repo}#{number}"))
}

fn queue(item: &str, tier: &str, parent: &str, brief: &str) -> NorthResult<String> {
    let item = canonical(item);
    if let Some(open) = open_follows_box(&item, brief)? {
        return Ok(open);
    }
    let mut child = Command::new(std::env::current_exe()?).args(queue_words(&item, tier, parent)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    std::io::Write::write_all(&mut child.stdin.take().ok_or_else(|| NorthError::State("queue follow has no stdin".into()))?, brief.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(NorthError::Refused(format!("overlap: {item} could not be queued: {}", String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn queue_words(item: &str, tier: &str, parent: &str) -> Vec<String> {
    ["threads", "queue", "follow", item, "--tier", tier, "--parent", parent].map(str::to_owned).to_vec()
}

fn open_follows_box(item: &str, brief: &str) -> NorthResult<Option<String>> {
    let output = Command::new(std::env::current_exe()?).args(["threads", "queue", "follow-list"]).stdin(Stdio::null()).output()?;
    Ok(same_box(&serde_json::from_slice(&output.stdout).unwrap_or(Value::Null), item, brief))
}

fn same_box(boxes: &Value, item: &str, brief: &str) -> Option<String> {
    boxes.as_array().into_iter().flatten().find(|found| found["item"] == item && found["brief"] == brief).map(|found| found["id"].to_string())
}

struct Repo {
    checkout: PathBuf,
    common: PathBuf,
}

impl Repo {
    fn of(directory: &Path) -> Option<Self> {
        let common = common_dir(directory)?;
        Some(Self { checkout: directory.to_path_buf(), common })
    }

    fn worktrees(&self) -> Option<PathBuf> {
        Some(self.common.parent()?.parent()?.join("worktrees"))
    }

    fn attributes(&self) -> Option<String> {
        git(&self.checkout, &["show", "origin/main:.gitattributes"])
    }
}

fn git(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(directory).args(args).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn common_dir(directory: &Path) -> Option<PathBuf> {
    let said = git(directory, &["rev-parse", "--git-common-dir"])?;
    std::fs::canonicalize(directory.join(said.trim())).ok()
}

pub(super) fn changed_files(lane: &Path) -> Option<BTreeSet<String>> {
    let base = git(lane, &["merge-base", "origin/main", "HEAD"])?;
    let mut files: BTreeSet<String> = git(lane, &["diff", "--name-only", base.trim()])?.lines().map(str::to_owned).collect();
    files.extend(git(lane, &["ls-files", "--others", "--exclude-standard"])?.lines().map(str::to_owned));
    Some(files)
}

fn ticket_files(root: &Path, head: &str) -> Option<BTreeSet<String>> {
    let base = git(root, &["merge-base", "origin/main", head])?;
    Some(git(root, &["diff", "--name-only", base.trim(), head])?.lines().map(str::to_owned).collect())
}

fn in_flight(fleet: &Fleet, repo: &Repo, own: &str) -> Vec<Lane> {
    let mut lanes: Vec<Lane> = Vec::new();
    let tasks: Vec<(String, Option<String>)> = fleet.sessions().unwrap_or_default().into_iter().map(|record| (record.name, record.task_ref)).collect();
    if let Some(worktrees) = repo.worktrees() {
        for running in fleet.running_lanes(&worktrees.to_string_lossy(), own) {
            let same_repo = common_dir(&running.path).is_some_and(|common| common == repo.common);
            let files = if same_repo { changed_files(&running.path) } else { None };
            let name = running.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            if let Some(files) = files.filter(|files| !files.is_empty()) {
                lanes.push(Lane { name, task: running.task, since_ms: running.since_ms, files });
            }
        }
    }
    for lane in ticket_lanes(repo, &tasks, &state_home()) {
        if !lanes.iter().any(|running| running.name == lane.name) {
            lanes.push(lane);
        }
    }
    lanes
}

fn ticket_lanes(repo: &Repo, tasks: &[(String, Option<String>)], state_home: &Path) -> Vec<Lane> {
    let Some(origin) = git(&repo.checkout, &["remote", "get-url", "--push", "origin"]) else { return Vec::new() };
    let directory = north_landing::land::queue::directory(state_home, origin.trim());
    let pending = north_landing::land::queue::pending(&directory).unwrap_or_default();
    pending
        .into_iter()
        .filter(|(_, ticket)| ticket.root.is_dir() && common_dir(&ticket.root).is_some_and(|common| common == repo.common))
        .filter_map(|(path, ticket)| {
            let files = ticket_files(&ticket.root, &ticket.head)?;
            let task = ticket.session.and_then(|session| tasks.iter().find(|(named, _)| *named == session).and_then(|(_, task)| task.clone()));
            Some(Lane { name: ticket.branch, task, since_ms: u64::try_from(north_landing::land::queue::queued_micros(&path) / 1000).unwrap_or(u64::MAX), files })
        })
        .collect()
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))).unwrap_or_default()
}

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;
