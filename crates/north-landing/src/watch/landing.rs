use std::collections::{BTreeMap, BTreeSet};

use super::facts::Event;
use super::{Alert, Severity, WatchContext};

pub const STALL_SECONDS: f64 = 120.0;
const CHECK_STAGES: [&str; 4] = ["land checks", "pre-push checks", "dependency gate and scans", "landing train"];
const TERMINAL: [&str; 4] = ["landed", "refused", "failed", "cancelled"];
const MIN_SAMPLES: usize = 5;
const QUEUE_FLOOR: f64 = 600.0;
const RECENT: f64 = 3600.0;
const PUSH_ERROR_WINDOW: f64 = 900.0;
const SETTLE_FRESH: u64 = 1800;

pub(super) fn probe(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    alerts.extend(stalls(context));
    alerts.extend(requeues(context));
    alerts.extend(queues(context));
    alerts.extend(push_errors(context));
    alerts.extend(shared_failures(context));
    alerts.extend(runtime(context));
    alerts
}

fn alert(key: String, severity: Severity, event: &Event, summary: String, since: f64) -> Alert {
    Alert { key, severity, repo: Some(event.repo.clone()), owner: (!event.session.is_empty()).then(|| event.session.clone()), summary, since: since as u64 }
}

fn latest_live(context: &WatchContext) -> BTreeMap<u32, &Event> {
    let mut latest: BTreeMap<u32, &Event> = BTreeMap::new();
    for event in context.events.iter().filter(|event| context.live.contains(&event.pid)) {
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
            if age > p90 * 1.5 {
                let summary = format!("{} lane {}: {} running {:.0} s, past its p90 {:.0} s + 50%", event.repo, event.lane, event.stage, age, p90);
                alerts.push(alert(format!("land:{}:overrun:{}", event.repo, event.lane), Severity::Warn, event, summary, event.time + p90 * 1.5));
            }
        }
    }
    alerts
}

fn requeues(context: &WatchContext) -> Vec<Alert> {
    let mut retries: BTreeMap<u32, Vec<&Event>> = BTreeMap::new();
    for event in context.events.iter().filter(|event| event.status == "retry" && context.live.contains(&event.pid)) {
        retries.entry(event.pid).or_default().push(event);
    }
    retries
        .into_values()
        .filter(|events| events.len() > 2)
        .filter_map(|events| {
            let (third, last) = (events.get(2)?, events.last()?);
            let summary = format!("{} lane {}: re-queued {} times, last at {} ({})", last.repo, last.lane, events.len(), last.stage, last.detail);
            Some(alert(format!("land:{}:requeue:{}", last.repo, last.lane), Severity::Warn, last, summary, third.time))
        })
        .collect()
}

fn queues(context: &WatchContext) -> Vec<Alert> {
    let mut queues: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    for ticket in context.tickets.iter().filter(|ticket| context.live.contains(&ticket.pid)) {
        queues.entry(ticket.queue.as_str()).or_default().push(ticket);
    }
    let mut alerts = Vec::new();
    for tickets in queues.values() {
        let Some(oldest) = tickets.iter().min_by(|a, b| a.time.total_cmp(&b.time)) else { continue };
        let repo = oldest.repo.as_str();
        let landed = context.events.iter().filter(|event| event.repo == repo && event.status == "landed").map(|event| event.time).fold(0.0, f64::max);
        let totals: Vec<f64> = context.stages.iter().filter(|sample| sample.repo == repo && sample.stage == "cleanup").map(|sample| sample.total).collect();
        let limit = (tickets.len() as f64 * percentile(&totals, 0.5).unwrap_or(0.0)).max(QUEUE_FLOOR);
        let since = oldest.time.max(landed);
        if context.now as f64 - since > limit {
            let summary = format!("{repo}: {} lane(s) queued to land and none has landed for {:.0} min (expected within {:.0} min); first in queue {}", tickets.len(), (context.now as f64 - since) / 60.0, limit / 60.0, oldest.lane);
            let owner = (!oldest.session.is_empty()).then(|| oldest.session.clone());
            alerts.push(Alert { key: format!("land:{repo}:queue-stalled"), severity: Severity::Page, repo: Some(repo.to_owned()), owner, summary, since: (since + limit) as u64 });
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
        for event in recent_land_check_failures(context, repo) {
            for test in &event.tests {
                if lanes.get(test.as_str()).is_none_or(|seen| seen.iter().all(|other| other.lane != event.lane)) {
                    lanes.entry(test).or_default().push(event);
                }
            }
        }
        for (test, events) in lanes.into_iter().filter(|(_, events)| events.len() >= 2) {
            let names: Vec<&str> = events.iter().map(|event| event.lane.as_str()).collect();
            let summary = format!("{repo}: possible red main: {test} failed the land checks of {} lanes ({})", names.len(), names.join(", "));
            alerts.push(alert(format!("land:{repo}:possible-red-main:{test}"), Severity::Page, events[0], summary, events[1].time));
        }
    }
    alerts
}

fn runtime(context: &WatchContext) -> Vec<Alert> {
    let Some(report) = &context.settle else { return Vec::new() };
    let (Some(landing), Some(checked)) = (report["landing"].as_str(), report["checkedAt"].as_u64()) else { return Vec::new() };
    if context.now.saturating_sub(checked) > SETTLE_FRESH {
        return Vec::new();
    }
    let failing: Vec<String> = report["hosts"].as_array().into_iter().flatten().filter_map(|host| {
        let problems: Vec<&str> = host["problems"].as_array()?.iter().filter_map(serde_json::Value::as_str).collect();
        (!problems.is_empty()).then(|| format!("{}: {}", host["host"].as_str().unwrap_or("?"), problems.join("; ")))
    }).collect();
    if failing.is_empty() {
        return Vec::new();
    }
    let short = &landing[..landing.len().min(12)];
    let landed = context.events.iter().find(|event| event.status == "landed" && event.sha == landing);
    let summary = format!("north landing {short} is not live and healthy past its live deadline: {}", failing.join(" | "));
    let owner = landed.and_then(|event| (!event.session.is_empty()).then(|| event.session.clone()));
    vec![Alert { key: format!("land:north:runtime-not-live:{short}"), severity: Severity::Warn, repo: Some("north".into()), owner, summary, since: landed.map_or(checked, |event| event.time as u64) }]
}
