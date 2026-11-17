use std::path::Path;

use super::facts::Repository;
use crate::land::candidate::{git, output};
use crate::land::lane_gates::{self, Lane};
use crate::land::scans::git::merge_base;
use crate::land::slots::runner::QueueRefusal;
use north_base::platform::finish_record::FinishRecords;
use north_diff_gates::dep_gate::Registry;
use north_landing::landing::{merge_set, stale_revert};

/// A lane the repository's checks before queueing judged and failed turns its session's passing finish record for
/// that tree failed, so a route's first rung moves to its second on the repository's own checks and not only on the
/// check its finish call named; a refusal that reached no verdict leaves the record as it is (north#1233). A missing review accept is no refusal: the
/// ticket waits for it in the landing queue and the record stays passing (north#1632). Each failed refusal's first line joins the record's `refused` list, where a repeat marks the lane stuck.
pub(super) fn refused_finish(session: Option<&str>, root: &Path, head: &str, refusal: QueueRefusal) -> String {
    let tree = |commit: &str| git(root, &["rev-parse", "--verify", "--quiet", &format!("{commit}^{{tree}}")]).ok();
    let records = FinishRecords::here();
    let own = std::env::var(north_base::platform::session_scope::SESSION).ok();
    let session_record = session.or(own.as_deref()).filter(|_| matches!(refusal, QueueRefusal::Failed(_))).and_then(|session| records.read(session));
    let first_line = refusal.text().lines().next().unwrap_or_default().to_owned();
    let failed = session_record.map(|mut finished| {
        if tree(&finished.head).is_some_and(|checked| Some(checked) == tree(head)) {
            finished.passed = false;
        }
        finished.refused.push(first_line);
        finished
    });
    if let Some(finished) = failed
        && let Err(error) = records.record(&finished)
    {
        eprintln!("safe-push: the finish record was not turned failed: {error}");
    }
    refusal.into_text()
}

pub(super) fn refuse_archived(repository: &Repository) -> Result<(), String> {
    if repository.archived {
        return Err(format!("GitHub reports {} as archived, so origin refuses every push; unarchive it or land elsewhere", repository.name));
    }
    Ok(())
}

pub(in crate::land) fn refuse_static_gates(root: &Path, destination: &str, head: &str) -> Result<(), &'static str> {
    let base = merge_base(root, destination, head);
    let base = base.as_str();
    if !north_diff_gates::language_gate::check(root, base, head) {
        return Err("language gate");
    }
    if !north_diff_gates::build_cache_gate::check(root, base, head) {
        return Err("build-cache gate");
    }
    if !north_diff_gates::dep_gate::check_reasons(root, base, head) {
        return Err("dependency gate");
    }
    if !north_diff_gates::stub_exec_gate::check(root, destination, base, head) {
        return Err("stub-exec gate");
    }
    if !north_diff_gates::secrets_gate::check(root, base, head) {
        return Err("secrets gate");
    }
    Ok(())
}

/// The names of the lane gates that would refuse the lane and their output, judged against the lander's post-land ledger when it lives on another host.
pub(super) fn refuse_lane_gates(root: &Path, base: &str, head: &str, state_home: &Path, lander_ledger: Option<String>, registry: &dyn Registry) -> Result<Result<(), (String, String)>, String> {
    let fork = merge_base(root, base, head);
    let borrowed = lander_ledger.map(|rows| ledger_home(&rows)).transpose().map_err(|error| format!("cannot keep the lander's post-land ledger: {error}"))?;
    let code_health = crate::land::after::gates::code_health_program();
    let advisory_db = north_diff_gates::advisory_gate::database(state_home);
    let lane = Lane { worktree: root, base: &fork, head, state_home: borrowed.as_ref().map_or(state_home, |home| home.path()), code_health: &code_health, registry, advisory_db: &advisory_db };
    let Some((failed, text)) = lane_gates::failed(&lane) else { return Ok(Ok(())) };
    let gates = failed.join(", ");
    let detail = format!("{gates} refused it here, as the lander would after its queue wait:\n{}", String::from_utf8_lossy(&text).trim_end());
    Ok(Err((gates, detail)))
}

fn judge_lane_gates(diff: &DiffOnly<'_>) -> Result<Option<String>, String> {
    let (root, base, head) = (diff.root, diff.base, diff.head);
    if let Err((gates, detail)) = refuse_lane_gates(root, base, head, diff.environment.state_home, diff.ledger.clone(), &north_diff_gates::dep_gate::PublicRegistries)? {
        diff.judged(&gates, Err(detail))?;
    }
    Ok(git(root, &["status", "--porcelain"]).is_ok_and(|status| status.is_empty()).then(|| merge_base(root, base, head)))
}

fn driver_merged<'a>(root: &Path, paths: &[&'a str]) -> Result<Vec<&'a str>, String> {
    let mut arguments = vec!["check-attr", "merge", "--"];
    arguments.extend(paths);
    let attributes = git(root, &arguments)?;
    let driven: Vec<&str> = attributes.lines().filter_map(|line| line.rsplit_once(": merge: ")).filter(|(_, driver)| merge_set::DRIVERS.iter().any(|known| known.name == *driver)).map(|(path, _)| path).collect();
    Ok(paths.iter().copied().filter(|path| driven.contains(path)).collect())
}

pub(super) fn refuse_conflict(root: &Path, base: &str, destination: &str) -> Result<(), String> {
    let merged = output(root, &["merge-tree", "--write-tree", "--name-only", "--no-messages", base, "HEAD"])?;
    if merged.status.code() != Some(1) {
        return Ok(());
    }
    let listing = String::from_utf8_lossy(&merged.stdout);
    let conflicted: Vec<&str> = listing.lines().skip(1).take_while(|line| !line.is_empty()).collect();
    let merged_by_driver = driver_merged(root, &conflicted)?;
    let left: Vec<&str> = conflicted.into_iter().filter(|path| !merged_by_driver.contains(path)).collect();
    if left.is_empty() {
        return Ok(());
    }
    let short = &base[..base.len().min(9)];
    Err(format!("it conflicts with origin/{destination} at {short} on {}, which the lander would send back as a conflict after its queue wait; rebase onto origin/{destination}, resolve those files and run safe-push again", left.join(", ")))
}

pub(super) fn refuse_stale_revert(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let Ok(candidate) = stale_revert::onto(root, base, head) else { return Ok(()) };
    match stale_revert::reverted(root, base, &candidate, head) {
        Ok(found) if !found.is_empty() => Err(format!("the revert guard refused it here, as the lander would after its queue wait:\n{}", stale_revert::refusal(&found))),
        _ => Ok(()),
    }
}

fn ledger_home(rows: &str) -> std::io::Result<tempfile::TempDir> {
    let home = tempfile::tempdir()?;
    std::fs::create_dir_all(home.path().join("safe-push"))?;
    std::fs::write(home.path().join("safe-push/post-land.tsv"), rows)?;
    Ok(home)
}

pub(super) fn refuse_nothing_to_land(root: &Path, base: &str, destination: &str) -> Result<(), String> {
    if !output(root, &["merge-base", "--is-ancestor", "HEAD", base])?.status.success() {
        return Ok(());
    }
    let status = output(root, &["status", "--porcelain", "-z", "--untracked-files=all"])?;
    let listing = String::from_utf8_lossy(&status.stdout);
    let paths: Vec<&str> = listing.split('\0').filter_map(|entry| entry.get(3..)).collect();
    let uncommitted = if paths.is_empty() { String::new() } else { format!(", and these paths are uncommitted: {}", paths.join(", ")) };
    Err(format!("nothing to land: HEAD is already on origin/{destination}, so the lane adds no commit{uncommitted}; commit the lane's change and run safe-push again"))
}

pub(super) struct DiffOnly<'a> {
    pub environment: &'a super::Environment<'a>,
    pub timings: &'a super::Timings,
    pub root: &'a Path,
    pub branch: &'a str,
    pub base: &'a str,
    pub head: &'a str,
    pub ledger: Option<String>,
    pub record: bool,
}

impl DiffOnly<'_> {
    fn judged(&self, gate: &str, outcome: Result<(), String>) -> Result<(), String> {
        if self.record {
            super::judged(self.environment, self.root, self.branch, self.head, gate, outcome)
        } else {
            north_diff_gates::landing_refusals::standing(gate, outcome)
        }
    }
}

pub(in crate::land::enqueue) fn confined_worker(confined_role: bool, cgroup: Option<&str>) -> bool {
    if confined_role {
        return true;
    }
    let Some(cgroup) = cgroup else { return true };
    let units: Vec<&str> = cgroup.lines().flat_map(|line| line.split('/')).collect();
    if units.iter().any(|unit| unit.starts_with(north_base::platform::session_scope::WORKER_BROKER_UNIT)) {
        return false;
    }
    let coordinator_held = format!("north-fleet-{}-", north_base::platform::session_scope::COORDINATOR_ROLE);
    let is_coordinator_held = units.iter().any(|unit| {
        unit.strip_prefix(&coordinator_held)
            .and_then(|rest| rest.strip_suffix(".service"))
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    });
    north_base::platform::session_scope::fleet_session_cgroup(cgroup) && !is_coordinator_held
}

pub(in crate::land::enqueue) fn confined_worker_here() -> bool {
    confined_worker(north_base::platform::session_scope::confined_session(), std::fs::read_to_string("/proc/self/cgroup").ok().as_deref())
}

pub(super) fn refuse_worker_ring_zero(root: &Path, base: &str, head: &str, confined_worker: bool) -> Result<(), String> {
    if !confined_worker {
        return Ok(());
    }
    let touched = north_land_scans::review_accept::ring_zero_touched(root, base, head)?;
    if touched.is_empty() {
        return Ok(());
    }
    Err(format!("ring-0 paths are write-denied to workers ({}); make this change from a session the owner types into", touched.join(", ")))
}

pub(super) fn refuse_diff_only(diff: DiffOnly<'_>) -> Result<Option<String>, String> {
    let (root, base, head, timings) = (diff.root, diff.base, diff.head, diff.timings);
    timings.stage("ring-0 write-denial", || refuse_worker_ring_zero(root, base, head, diff.environment.confined_worker))?;
    timings.stage("generated registry", || crate::land::scans::generated_registry::check(root, base, head))?;
    if let Err(gate) = timings.stage("static gates", || refuse_static_gates(root, base, head)) {
        diff.judged(gate, Err(format!("{gate} refused the landing")))?;
    }
    let bodies = git(root, &["log", "--format=%B", &format!("{base}..{head}")])?;
    diff.judged("commit trailers", crate::land::divergence::refuse_malformed(&bodies))?;
    let gated_from = timings.stage("lane gates", || judge_lane_gates(&diff))?;
    diff.judged("revert guard", timings.stage("revert guard", || refuse_stale_revert(root, base, head)))?;
    Ok(gated_from)
}
