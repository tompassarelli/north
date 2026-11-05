use super::{Alert, Probe, Severity, WatchContext};

pub const PROBES: [Probe; 6] = [capacity, workers, waits, runtime_follow, hosts, super::waste::waste];

pub(super) const QUEUE_FLOOR_SECS: u64 = 5 * 60;
pub(super) const PRESSURE_HOLD_SECS: u64 = 2 * 60;
pub(super) const SILENT_SECS: u64 = 10 * 60;
pub(super) const DEFAULT_ETA_MIN: u64 = 45;
pub(super) const ABANDON_WINDOW_SECS: u64 = 30 * 60;
pub(super) const ABANDON_LIMIT: usize = 3;
pub(super) const STALE_RUNTIME_SECS: u64 = 15 * 60;
pub(super) const UNREACHABLE_SECS: u64 = 2 * 60;

fn alert(key: String, severity: Severity, summary: String, since: u64) -> Alert {
    Alert { key, severity, repo: None, owner: None, summary, since, repeat_secs: None }
}

fn minutes(seconds: u64) -> u64 {
    seconds / 60
}

pub fn capacity(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for host in &context.hosts {
        let Some(view) = &host.capacity else { continue };
        for ticket in &view.queued {
            let waited = context.now.saturating_sub(ticket.queued_since);
            let limit = ticket.wait_p90_secs.unwrap_or(0).max(QUEUE_FLOOR_SECS);
            if waited > limit {
                let summary = format!("{} ticket of {} queued {} min on {} (limit {} min)", ticket.class, ticket.owner, minutes(waited), host.name, minutes(limit));
                alerts.push(Alert { owner: Some(ticket.owner.clone()), ..alert(format!("work:capacity:{}:queue:{}", host.name, ticket.owner), Severity::Warn, summary, ticket.queued_since) });
            }
        }
        if let Some(since) = view.pressure_since.filter(|since| host.local && !view.queued.is_empty() && context.now.saturating_sub(*since) >= PRESSURE_HOLD_SECS) {
            let summary = format!("CPU pressure some avg10 {:.1}% above the {:.0}% target for {} min with {} tickets queued", view.cpu_some_avg10, view.pressure_target, minutes(context.now - since), view.queued.len());
            alerts.push(alert(format!("work:capacity:{}:pressure", host.name), Severity::Warn, summary, since));
        }
    }
    alerts
}

pub fn workers(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for session in context.sessions.iter().filter(|session| session.status == "working") {
        let eta = session.eta_min.unwrap_or(DEFAULT_ETA_MIN);
        let worked = context.now.saturating_sub(session.working_since);
        let worker = |kind: &str, summary: String, since: u64| Alert { repo: session.repo.clone(), owner: session.parent.clone(), ..alert(format!("work:worker:{}:{kind}", session.name), Severity::Warn, summary, since) };
        if worked > 2 * eta * 60 {
            alerts.push(worker("overrun", format!("{} working {} min against an ETA of {eta} min", session.name, minutes(worked)), session.working_since));
        }
        if let Some(last) = session.last_event.filter(|last| !session.waiting && context.now.saturating_sub(*last) >= SILENT_SECS) {
            alerts.push(worker("silent", format!("{} working but silent {} min with no registered wait", session.name, minutes(context.now - last)), last));
        }
    }
    for cloud in context.cloud.sessions.iter().filter(|cloud| cloud.status == "running" && context.now.saturating_sub(cloud.updated) >= SILENT_SECS) {
        let summary = format!("cloud session {} ({}) running but silent {} min", cloud.id, cloud.title, minutes(context.now - cloud.updated));
        alerts.push(alert(format!("work:cloud:{}:silent", cloud.id), Severity::Warn, summary, cloud.updated));
    }
    if let Some(failure) = &context.cloud.failure {
        alerts.push(alert("work:cloud:list".into(), Severity::Info, format!("north cloud list failed, cloud sessions unwatched: {failure}"), context.now));
    }
    for agent in &context.subagents {
        let Some(finished) = agent.shell_finished.filter(|_| context.now.saturating_sub(agent.last_event) >= SILENT_SECS) else { continue };
        let summary = format!("subagent {} of session {} silent {} min after a background shell it started finished", agent.agent, agent.session, minutes(context.now - agent.last_event));
        alerts.push(Alert { repo: agent.repo.clone(), owner: agent.owner.clone(), ..alert(format!("work:subagent:{}:unwoken", agent.agent), Severity::Warn, summary, finished) });
    }
    alerts
}

pub(super) fn expected_secs(run: &str) -> u64 {
    let minutes = match run.split_once(':').map_or("", |(kind, _)| kind) {
        "farm" => 45,
        "capture" => 30,
        _ => 60,
    };
    minutes * 60
}

pub fn waits(context: &WatchContext) -> Vec<Alert> {
    context
        .waits
        .iter()
        .filter(|wait| context.now.saturating_sub(wait.since) > expected_secs(&wait.run))
        .map(|wait| {
            let summary = format!("{} waited {} min on {} (expected {} min)", wait.session, minutes(context.now - wait.since), wait.run, minutes(expected_secs(&wait.run)));
            Alert { repo: wait.repo.clone(), owner: Some(wait.session.clone()), ..alert(format!("work:wait:{}:{}", wait.session, wait.run), Severity::Warn, summary, wait.since) }
        })
        .collect()
}

pub fn runtime_follow(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    let recent: Vec<u64> = context.runtime.abandoned.iter().copied().filter(|at| context.now.saturating_sub(*at) <= ABANDON_WINDOW_SECS).collect();
    if recent.len() > ABANDON_LIMIT {
        let summary = format!("runtime-follow abandoned {} builds in the last 30 min", recent.len());
        alerts.push(Alert { repo: Some("north".into()), ..alert("work:runtime:abandoned".into(), Severity::Warn, summary, recent.iter().copied().min().unwrap_or(context.now)) });
    }
    if let Some(since) = context.runtime.rust_change_since.filter(|since| context.now.saturating_sub(*since) > STALE_RUNTIME_SECS) {
        let live = context.runtime.live.as_deref().map_or("unknown", |live| live.get(..8).unwrap_or(live));
        let summary = format!("live north runtime {live} is {} min behind a Rust change on origin/main", minutes(context.now - since));
        alerts.push(Alert { repo: Some("north".into()), ..alert("work:runtime:stale".into(), Severity::Warn, summary, since) });
    }
    alerts
}

pub fn hosts(context: &WatchContext) -> Vec<Alert> {
    context
        .hosts
        .iter()
        .filter_map(|host| {
            let since = host.down_since.filter(|since| context.now.saturating_sub(*since) >= UNREACHABLE_SECS)?;
            let summary = format!("{} unreachable over ssh for {} min", host.name, minutes(context.now - since));
            Some(alert(format!("work:host:{}:unreachable", host.name), Severity::Page, summary, since))
        })
        .collect()
}
