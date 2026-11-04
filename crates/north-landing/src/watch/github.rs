use std::collections::BTreeMap;

use super::facts::{RepoFacts, Run};
use super::landing::recent_land_check_failures;
use super::{Alert, Severity, WatchContext};

const QUEUED_SECONDS: u64 = 120;
const AUTOLAND_FAILURE_WINDOW: u64 = 1800;
const AUTOLAND_IDLE_SECONDS: u64 = 600;
const FAILED: [&str; 3] = ["failure", "timed_out", "startup_failure"];
const UNJUDGED: [&str; 3] = ["cancelled", "skipped", "neutral"];

pub(super) fn probe(context: &WatchContext) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for repo in &context.repos {
        if let Some(error) = &repo.error {
            alerts.push(raise(repo, "api-error", Severity::Warn, None, format!("{}: GitHub API or git error: {error}", repo.name), context.now));
            continue;
        }
        alerts.extend(red_main(context, repo));
        alerts.extend(queued(context, repo));
        alerts.extend(autoland(context, repo));
    }
    alerts
}

pub fn at(text: &str) -> Option<u64> {
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let month = time::Month::try_from(u8::try_from(month).ok()?).ok()?;
    let date = time::Date::from_calendar_date(i32::try_from(year).ok()?, month, u8::try_from(day).ok()?).ok()?;
    let days = i64::from(date.to_julian_day()) - 2_440_588;
    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

fn raise(repo: &RepoFacts, kind: &str, severity: Severity, owner: Option<String>, summary: String, since: u64) -> Alert {
    Alert { key: format!("gh:{}:{kind}", repo.name), severity, repo: Some(repo.name.clone()), owner, summary, since }
}

fn owner_of_sha(context: &WatchContext, sha: &str) -> Option<String> {
    context.events.iter().rev().find(|event| event.status == "landed" && event.sha == sha && !event.session.is_empty()).map(|event| event.session.clone())
}

fn red_main(context: &WatchContext, repo: &RepoFacts) -> Vec<Alert> {
    let mut latest: BTreeMap<&str, &Run> = BTreeMap::new();
    let judged = repo.runs.iter().filter(|run| run.head_branch == "main" && run.event == "push" && run.status == "completed");
    for run in judged.filter(|run| run.conclusion.as_deref().is_some_and(|conclusion| !UNJUDGED.contains(&conclusion))) {
        if latest.get(run.name.as_str()).is_none_or(|seen| seen.created_at < run.created_at) {
            latest.insert(&run.name, run);
        }
    }
    let lanes: Vec<String> = recent_land_check_failures(context, &repo.name).map(|event| event.lane.clone()).collect();
    let mut alerts = Vec::new();
    for (workflow, run) in latest.into_iter().filter(|(_, run)| run.conclusion.as_deref().is_some_and(|conclusion| FAILED.contains(&conclusion))) {
        let short = &run.head_sha[..run.head_sha.len().min(12)];
        let mut summary = format!("{} main is red: {workflow} failed on main commit {short} ({})", repo.name, run.html_url);
        if !lanes.is_empty() {
            summary.push_str(&format!("; land checks also failed in the last hour for {}", lanes.join(", ")));
        }
        let since = at(&run.updated_at).unwrap_or(context.now);
        alerts.push(raise(repo, &format!("red-main:{workflow}"), Severity::Page, owner_of_sha(context, &run.head_sha), summary, since));
    }
    alerts
}

fn queued(context: &WatchContext, repo: &RepoFacts) -> Option<Alert> {
    let waiting: Vec<(u64, &Run)> = repo.runs.iter().filter(|run| matches!(run.status.as_str(), "queued" | "waiting" | "pending" | "requested")).filter_map(|run| Some((at(&run.created_at)?, run))).filter(|(created, _)| context.now.saturating_sub(*created) > QUEUED_SECONDS).collect();
    let (created, oldest) = waiting.iter().min_by_key(|(created, _)| *created)?;
    let summary = format!("{}: {} Actions run(s) queued over {QUEUED_SECONDS} s, oldest {} s ({} on {})", repo.name, waiting.len(), context.now - created, oldest.name, oldest.head_branch);
    Some(raise(repo, "queued", Severity::Warn, None, summary, created + QUEUED_SECONDS))
}

fn autoland(context: &WatchContext, repo: &RepoFacts) -> Vec<Alert> {
    if !repo.autoland {
        return Vec::new();
    }
    let runs: Vec<&Run> = repo.runs.iter().filter(|run| run.path.ends_with("autoland.yml")).collect();
    let mut alerts = Vec::new();
    for run in runs.iter().filter(|run| run.conclusion.as_deref().is_some_and(|conclusion| FAILED.contains(&conclusion))) {
        let Some(updated) = at(&run.updated_at).filter(|updated| context.now.saturating_sub(*updated) <= AUTOLAND_FAILURE_WINDOW) else { continue };
        let newer = runs.iter().any(|other| other.head_branch == run.head_branch && other.created_at > run.created_at);
        if !newer {
            let summary = format!("{}: Autoland run failed for {} ({})", repo.name, run.head_branch, run.html_url);
            alerts.push(raise(repo, &format!("autoland-failed:{}", run.head_branch), Severity::Warn, None, summary, updated));
        }
    }
    let busy = runs.iter().any(|run| run.status != "completed");
    let Some(last) = runs.iter().filter_map(|run| at(&run.updated_at)).max() else { return alerts };
    if !repo.land_branches.is_empty() && !busy && context.now.saturating_sub(last) > AUTOLAND_IDLE_SECONDS {
        let summary = format!("{}: {} claude/land-* branch(es) waiting and no Autoland run for {} min", repo.name, repo.land_branches.len(), context.now.saturating_sub(last) / 60);
        alerts.push(raise(repo, "autoland-stall", Severity::Page, None, summary, last + AUTOLAND_IDLE_SECONDS));
    }
    alerts
}
