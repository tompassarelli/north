use std::collections::{BTreeMap, BTreeSet};

use north_base::platform::harness::default_number;
use north_base::platform::watch_named::run_time as at;

use super::facts::{Event, Run};
use super::{Alert, Detail, Severity, WatchContext};

pub const STALL_SECONDS: f64 = 120.0;
const CHECK_STAGES: [&str; 2] = ["land checks", "dependency gate and scans"];
const TERMINAL: [&str; 4] = ["landed", "refused", "failed", "cancelled"];
const OVERRUN_FLOOR: &str = "landing_watch_overrun_floor_seconds";
const MIN_SAMPLES: usize = 5;
const RECENT: f64 = 3600.0;
const PUSH_ERROR_WINDOW: f64 = 900.0;
const SETTLE_FRESH: u64 = 1800;
const FOLLOW_PROGRESS_SECONDS: u64 = 15 * 60;
const TEST_WORKFLOW: &str = "Cargo";

pub(super) fn probe(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    alerts.extend(stalls(context));
    alerts.extend(push_errors(context));
    alerts.extend(shared_failures(context));
    if !context.defers_fleet_alarms {
        alerts.extend(runtime(context));
    }
    alerts
}

fn alert(key: String, severity: Severity, event: &Event, summary: String, since: f64) -> Alert {
    Alert { key, severity, repo: Some(event.repo.clone()), owner: (!event.session.is_empty()).then(|| event.session.clone()), summary, since: since as u64, detail: Detail::default() }
}

fn latest_live(context: &WatchContext) -> BTreeMap<u32, &Event> {
    let mut latest: BTreeMap<u32, &Event> = BTreeMap::new();
    for event in context.events.iter().filter(|event| context.live.contains(&event.pid) && !event.lane.is_empty()) {
        if latest.get(&event.pid).is_none_or(|seen| seen.time <= event.time) {
            latest.insert(event.pid, event);
        }
    }
    latest.retain(|_, event| !TERMINAL.contains(&event.status.as_str()));
    latest
}

pub fn percentile(samples: &[f64], fraction: f64) -> Option<f64> {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * fraction).ceil() as usize).checked_sub(1)?;
    sorted.get(index).copied()
}

fn stage_samples(context: &WatchContext, repo: &str, stage: &str) -> Vec<f64> {
    let history = context.stages.iter().filter(|sample| sample.stage == stage && (sample.repo == repo || sample.repo.is_empty())).map(|sample| sample.seconds);
    history.filter(|seconds| *seconds > 0.0).collect()
}

fn stalls(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for event in latest_live(context).into_values() {
        let age = context.now as f64 - event.time;
        if event.status != "wait" && age >= STALL_SECONDS {
            let summary = format!("{} lane {}: safe-push has written no event for {:.0} s since {} {}", event.repo, event.lane, age, event.stage, event.status);
            alerts.push(alert(format!("land:{}:stall:{}", event.repo, event.lane), Severity::Page, event, summary, event.time));
        } else if event.status == "wait" && CHECK_STAGES.contains(&event.stage.as_str()) {
            let samples = stage_samples(context, &event.repo, &event.stage);
            let Some(p90) = percentile(&samples, 0.9).filter(|_| samples.len() >= MIN_SAMPLES) else { continue };
            let limit = (p90 * 1.5).max(default_number(OVERRUN_FLOOR));
            if age > limit {
                let summary = format!("{} lane {}: {} running {:.0} s, past its p90 {:.0} s + 50% and the {:.0} s floor", event.repo, event.lane, event.stage, age, p90, default_number(OVERRUN_FLOOR));
                alerts.push(alert(format!("land:{}:overrun:{}", event.repo, event.lane), Severity::Warn, event, summary, event.time + limit));
            }
        }
    }
    alerts
}

fn push_errors(context: &WatchContext) -> Vec<Alert> {
    let now = context.now as f64;
    let failed = context.events.iter().filter(|event| event.stage == "push" && event.status == "failed" && now - event.time <= PUSH_ERROR_WINDOW);
    failed
        .filter(|failure| !context.events.iter().any(|later| later.repo == failure.repo && later.lane == failure.lane && later.status == "landed" && later.time > failure.time))
        .map(|failure| {
            let summary = format!("{} lane {}: GitHub refused the push: {}", failure.repo, failure.lane, failure.detail);
            alert(format!("land:{}:push-error:{}", failure.repo, failure.lane), Severity::Warn, failure, summary, failure.time)
        })
        .collect()
}

pub fn recent_land_check_failures<'a>(context: &'a WatchContext, repo: &'a str) -> impl Iterator<Item = &'a Event> {
    let now = context.now as f64;
    context.events.iter().filter(move |event| event.repo == repo && event.status == "failed" && event.stage.starts_with("land check") && now - event.time <= RECENT)
}

fn shared_failures(context: &WatchContext) -> Vec<Alert> {
    let repos: BTreeSet<&str> = context.events.iter().map(|event| event.repo.as_str()).collect();
    let mut alerts = Vec::new();
    for repo in repos {
        let mut lanes: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
        for event in recent_land_check_failures(context, repo).filter(|event| !event.stale_base) {
            for test in &event.tests {
                if lanes.get(test.as_str()).is_none_or(|seen| seen.iter().all(|other| other.lane != event.lane)) {
                    lanes.entry(test).or_default().push(event);
                }
            }
        }
        let passed_since = |time: f64| main_passed_since(context, repo, time);
        for (test, events) in lanes.into_iter().filter(|(_, events)| events.len() >= 2) {
            if passed_since(events.iter().map(|event| event.time).fold(f64::MIN, f64::max)) {
                continue;
            }
            let mut raised = alert(format!("land:{repo}:possible-red-main:{test}"), Severity::Page, events[0], format!("{repo}: possible red main: {test}"), events[1].time);
            raised.detail = Detail { lead: "it failed the land checks of".into(), unit: "lanes".into(), items: distinct_lanes(events.iter().copied()) };
            alerts.push(raised);
        }
    }
    alerts
}

fn main_passed_since(context: &WatchContext, repo: &str, time: f64) -> bool {
    let passing = |run: &&Run| run.name == TEST_WORKFLOW && run.head_branch == "main" && run.event == "push" && run.status == "completed" && run.conclusion.as_deref() == Some("success");
    let created = |run: &Run| at(&run.created_at).map(|created| created as f64);
    context.repos.iter().filter(|facts| facts.name == repo).flat_map(|facts| facts.runs.iter()).filter(passing).any(|run| created(run).is_some_and(|created| created > time))
}

fn runtime(context: &WatchContext) -> Vec<Alert> {
    let Some(report) = &context.settle else { return Vec::new() };
    let (Some(landing), Some(checked)) = (report["landing"].as_str(), report["checkedAt"].as_u64()) else { return Vec::new() };
    if context.now.saturating_sub(checked) > SETTLE_FRESH {
        return Vec::new();
    }
    let hosts = || report["hosts"].as_array().into_iter().flatten();
    let name = |host: &serde_json::Value| host["host"].as_str().unwrap_or("?").to_owned();
    let failing: Vec<String> = hosts().filter_map(|host| {
        let problems: Vec<&str> = host["problems"].as_array()?.iter().filter_map(serde_json::Value::as_str).collect();
        (!problems.is_empty()).then(|| format!("{}: {}", name(host), problems.join("; ")))
    }).collect();
    let short = &landing[..landing.len().min(12)];
    if failing.is_empty() {
        let held: Vec<String> = hosts().filter_map(|host| Some(format!("{} stays on its runtime, held by {}", name(host), host["held"].as_str()?))).collect();
        return (!held.is_empty()).then(|| Alert { key: format!("land:north:runtime-held:{short}"), severity: Severity::Info, repo: Some("north".into()), owner: None, summary: format!("north landing {short} is live and healthy on every host but the held: {}", held.join(" | ")), since: checked, detail: Detail::default() }).into_iter().collect();
    }
    let landed = context.events.iter().find(|event| event.status == "landed" && event.sha == landing);
    let following = context.follow.as_ref().filter(|follow| follow.contains_landing);
    let held = following.is_some_and(|follow| context.now.saturating_sub(follow.at) < FOLLOW_PROGRESS_SECONDS);
    let stalled = if held || following.is_none() { "" } else { "; the runtime follower has made no progress for 15 min" };
    let summary = format!("north landing {short} is not live and healthy past its live deadline{stalled}: {}", failing.join(" | "));
    let owner = landed.and_then(|event| (!event.session.is_empty()).then(|| event.session.clone()));
    let severity = if held { Severity::Info } else { Severity::Warn };
    vec![Alert { key: format!("land:north:runtime-not-live:{short}"), severity, repo: Some("north".into()), owner, summary, since: landed.map_or(checked, |event| event.time as u64), detail: Detail::default() }]
}

#[cfg(test)]
#[path = "landing_tests.rs"]
mod tests;

/// The distinct lanes of `events` in first-seen order; a probe's lane field lists its lanes comma-separated.
pub(super) fn distinct_lanes<'a>(events: impl Iterator<Item = &'a Event>) -> Vec<String> {
    let mut lanes: Vec<String> = Vec::new();
    for lane in events.flat_map(|event| event.lane.split(',')).map(str::trim).filter(|lane| !lane.is_empty()) {
        if !lanes.iter().any(|seen| seen == lane) {
            lanes.push(lane.to_owned());
        }
    }
    lanes
}
