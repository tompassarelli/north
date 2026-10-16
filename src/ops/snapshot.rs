use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use super::expect::{self, Expectation, Outcome, State};
use super::facts::{DAY, Facts, IssueKind, Load, LockHolder, Place, WEEK, parse};
use crate::foundation::clock::Timestamp;

const QUOTA: f64 = 100.0;

#[derive(Debug, Serialize)]
pub(crate) struct Snapshot {
    pub at: String,
    pub accounts: Vec<AccountRow>,
    pub placement: Vec<PlaceRow>,
    pub landing: Vec<QueueRow>,
    pub release: ReleaseRow,
    pub bets: Vec<BetRow>,
    pub load: Option<Load>,
    pub expectations: Vec<Outcome>,
    pub breached: Vec<String>,
    pub gaps: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AccountRow {
    pub account: String,
    pub five_hour_pct: Option<f64>,
    pub week_pct: Option<f64>,
    pub burn_pct_per_hour: Option<f64>,
    pub hours_to_exhaustion: Option<f64>,
    pub exhausts: Option<String>,
    pub five_hour_resets_at: Option<String>,
    pub sessions_live: usize,
    pub sessions_working: usize,
    pub sampled_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlaceRow {
    pub place: Place,
    pub label: &'static str,
    pub sessions_24h: Option<usize>,
    pub cpu_hours_24h: Option<f64>,
    pub cpu_hours_7d: Option<f64>,
    pub measure: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct WaiterRow {
    pub session: Option<String>,
    pub branch: String,
    pub waited_minutes: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct QueueRow {
    pub repo: String,
    pub holder: Option<LockHolder>,
    pub held_minutes: Option<f64>,
    pub waiters: Vec<WaiterRow>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReleaseRow {
    pub milestone: &'static str,
    pub open_work: Option<usize>,
    pub closed_work: Option<usize>,
    pub closed_7d: Option<usize>,
    pub builds_7d: usize,
    pub landings_24h: usize,
    pub landings_7d: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct BetRow {
    pub bet: u8,
    pub name: &'static str,
    pub metric: &'static str,
    pub value: Option<f64>,
    pub unit: &'static str,
    pub detail: String,
}

pub(crate) fn assemble(facts: &Facts, expectations: &[Expectation]) -> Snapshot {
    let outcomes: Vec<Outcome> = expectations.iter().map(|expectation| expect::evaluate(expectation, facts)).collect();
    let accounts = accounts(facts);
    let landing = landing(facts);
    Snapshot {
        at: Timestamp::from_unix_seconds(facts.now).iso(),
        placement: placement(facts),
        release: release(facts),
        bets: bets(facts, &landing),
        load: facts.load,
        gaps: facts.gaps.clone(),
        accounts,
        landing,
        breached: outcomes.iter().filter(|outcome| outcome.state == State::Breached).map(|outcome| outcome.id.clone()).collect(),
        expectations: outcomes,
    }
}

fn accounts(facts: &Facts) -> Vec<AccountRow> {
    let Some(table) = facts.pace.as_ref().and_then(|pace| pace["accounts"].as_object()) else { return Vec::new() };
    table
        .iter()
        .filter(|(name, _)| name.starts_with("claude:"))
        .map(|(name, entry)| account(facts, name, entry))
        .collect()
}

fn account(facts: &Facts, name: &str, entry: &Value) -> AccountRow {
    let windows: BTreeMap<&str, &Value> = entry["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|window| Some((window["window"].as_str()?, window)))
        .collect();
    let used = |window: &str| windows.get(window).and_then(|w| w["used_pct"].as_f64());
    let exhaustion = windows
        .iter()
        .filter_map(|(label, window)| Some((*label, hours_to_exhaustion(window, facts.now)?)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let sessions = facts.sessions.iter().filter(|session| session.account == name);
    AccountRow {
        account: name.to_owned(),
        five_hour_pct: used("5h"),
        week_pct: used("week"),
        burn_pct_per_hour: windows.get("5h").and_then(|w| w["burn_pct_h"].as_f64()),
        hours_to_exhaustion: exhaustion.map(|(_, hours)| hours),
        exhausts: exhaustion.map(|(label, _)| label.to_owned()),
        five_hour_resets_at: windows.get("5h").and_then(|w| w["resets_at"].as_str()).map(str::to_owned),
        sessions_live: sessions.clone().filter(|session| session.live).count(),
        sessions_working: sessions.filter(|session| session.working).count(),
        sampled_at: entry["at"].as_f64().map(|at| Timestamp::from_unix_seconds_f64(at).iso()),
    }
}

pub(crate) fn hours_to_exhaustion(window: &Value, now: i64) -> Option<f64> {
    let used = window["used_pct"].as_f64()?;
    let burn = window["burn_pct_h"].as_f64().filter(|burn| *burn > 0.0)?;
    let hours = ((QUOTA - used) / burn).max(0.0);
    let reset = window["resets_at"].as_str().and_then(parse::seconds).map(|at| (at - now) as f64 / 3600.0);
    reset.is_none_or(|reset| hours < reset).then_some(hours)
}

fn placement(facts: &Facts) -> Vec<PlaceRow> {
    Place::ALL.into_iter().map(|place| place_row(facts, place)).collect()
}

fn place_row(facts: &Facts, place: Place) -> PlaceRow {
    let day = facts.now - DAY;
    let cpu = |since: i64| cpu_hours(facts, place, since, facts.now);
    let (sessions, measure) = match place {
        Place::Laptop => (
            Some(facts.sessions.iter().filter(|session| session.live || session.created_at >= day).count()),
            "fleet sessions; CPU of machine-capacity runs",
        ),
        Place::Nexus => (
            Some(facts.runs.iter().filter(|run| run.place == place && run.at >= day).map(|run| run.owner.as_str()).collect::<BTreeSet<_>>().len()),
            "capacity-run owners; CPU of machine-capacity runs",
        ),
        Place::HostedFarm => (facts.farm.as_ref().map(|runs| runs.iter().filter(|run| run.created_at >= day).count()), "farm-test runs; runner job-hours"),
        Place::AnthropicCloud => (facts.cloud.as_ref().map(|cloud| cloud.iter().filter(|session| session.placed_at >= day).count()), "cloud sessions placed; CPU not reported"),
    };
    PlaceRow { place, label: place.label(), sessions_24h: sessions, cpu_hours_24h: cpu(day), cpu_hours_7d: cpu(facts.now - WEEK), measure }
}

pub(crate) fn cpu_hours(facts: &Facts, place: Place, from: i64, to: i64) -> Option<f64> {
    let within = |at: i64| at >= from && at < to;
    let seconds: f64 = match place {
        Place::Laptop => facts.runs.iter().filter(|run| run.place == place && within(run.at)).map(|run| run.cpu_seconds).sum(),
        Place::Nexus if !facts.nexus => return None,
        Place::Nexus => facts.runs.iter().filter(|run| run.place == place && within(run.at)).map(|run| run.cpu_seconds).sum(),
        Place::HostedFarm => facts.farm.as_ref()?.iter().filter(|run| within(run.created_at)).map(|run| run.job_seconds).sum(),
        Place::AnthropicCloud => return None,
    };
    Some(seconds / 3600.0)
}

fn landing(facts: &Facts) -> Vec<QueueRow> {
    let minutes = |since: i64| (facts.now - since).max(0) as f64 / 60.0;
    facts
        .locks
        .iter()
        .filter(|lock| lock.holder.is_some() || !lock.waiters.is_empty())
        .map(|lock| QueueRow {
            repo: lock.repo.clone(),
            held_minutes: lock.holder.as_ref().map(|_| minutes(lock.held_since)),
            holder: lock.holder.clone(),
            waiters: lock
                .waiters
                .iter()
                .map(|waiter| WaiterRow { session: waiter.session.clone(), branch: waiter.branch.clone(), waited_minutes: minutes(waiter.since) })
                .collect(),
        })
        .collect()
}

fn release(facts: &Facts) -> ReleaseRow {
    let work: Option<Vec<_>> = facts.release.as_ref().map(|issues| issues.iter().filter(|issue| issue.kind == IssueKind::Work).collect());
    let count = |keep: &dyn Fn(&&&super::facts::ReleaseIssue) -> bool| work.as_ref().map(|issues| issues.iter().filter(keep).count());
    ReleaseRow {
        milestone: "Public 0.1",
        open_work: count(&|issue| issue.open),
        closed_work: count(&|issue| !issue.open),
        closed_7d: count(&|issue| !issue.open && issue.closed_at.is_some_and(|at| at >= facts.now - WEEK)),
        builds_7d: facts.builds.len(),
        landings_24h: facts.landings.iter().filter(|landing| landing.at >= facts.now - DAY).count(),
        landings_7d: facts.landings.len(),
    }
}

fn bets(facts: &Facts, landing: &[QueueRow]) -> Vec<BetRow> {
    let open = release(facts).open_work;
    let (median, per_repo) = commit_to_land(facts);
    let longest_wait = landing.iter().flat_map(|row| row.waiters.iter().map(|waiter| waiter.waited_minutes)).fold(0.0_f64, f64::max);
    vec![
        BetRow {
            bet: 1,
            name: "Ship Public 0.1",
            metric: "open release work items",
            value: open.map(|open| open as f64),
            unit: "items",
            detail: "milestone Public 0.1 without epics and Tom-only items; target 0".into(),
        },
        BetRow {
            bet: 2,
            name: "Test drag down",
            metric: "median minutes from commit to landed, 7 d",
            value: median,
            unit: "min",
            detail: format!("{per_repo}; longest landing-queue wait now {longest_wait:.0} min"),
        },
        BetRow {
            bet: 3,
            name: "Ask to land, first pass",
            metric: "hours from Tom's ask to landed",
            value: None,
            unit: "h",
            detail: "not instrumented yet: asks are not linked to landings".into(),
        },
    ]
}

fn commit_to_land(facts: &Facts) -> (Option<f64>, String) {
    let mut by_repo: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for landing in &facts.landings {
        if let Some(committed) = landing.committed_at {
            by_repo.entry(&landing.repo).or_default().push((landing.at - committed).max(0) as f64 / 60.0);
        }
    }
    let mut all: Vec<f64> = by_repo.values().flatten().copied().collect();
    let detail = by_repo
        .iter_mut()
        .map(|(repo, minutes)| format!("{repo} {:.0}", median(minutes).unwrap_or(0.0)))
        .collect::<Vec<_>>()
        .join(", ");
    (median(&mut all), detail)
}

pub(crate) fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(values[n / 2]),
        _ => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
    }
}

pub(crate) fn summary(snapshot: &Snapshot) -> String {
    let mut lines = vec![format!("north ops snapshot {}", snapshot.at)];
    for account in &snapshot.accounts {
        lines.push(format!(
            "account {}: 5h {}%, week {}%, burn {}/h, exhausts {}, {} live / {} working sessions",
            account.account,
            number(account.five_hour_pct),
            number(account.week_pct),
            number(account.burn_pct_per_hour),
            account.hours_to_exhaustion.map_or("after reset".to_owned(), |hours| format!("in {hours:.1} h ({})", account.exhausts.as_deref().unwrap_or("?"))),
            account.sessions_live,
            account.sessions_working
        ));
    }
    for place in &snapshot.placement {
        lines.push(format!("{}: {} sessions/24h, {} CPU-h/24h, {} CPU-h/7d ({})", place.label, place.sessions_24h.map_or("?".into(), |n| n.to_string()), number(place.cpu_hours_24h), number(place.cpu_hours_7d), place.measure));
    }
    for queue in &snapshot.landing {
        let holder = queue.holder.as_ref().map_or("free".to_owned(), |holder| format!("held {:.0} min by {}", queue.held_minutes.unwrap_or(0.0), holder.session.as_deref().unwrap_or(&holder.dir)));
        lines.push(format!("landing {}: {holder}, {} waiting", queue.repo, queue.waiters.len()));
    }
    lines.push(format!(
        "release {}: {} open work items, {} closed in 7 d, {} builds, {} landings/24h, {} landings/7d",
        snapshot.release.milestone,
        snapshot.release.open_work.map_or("?".into(), |n| n.to_string()),
        snapshot.release.closed_7d.map_or("?".into(), |n| n.to_string()),
        snapshot.release.builds_7d,
        snapshot.release.landings_24h,
        snapshot.release.landings_7d
    ));
    for bet in &snapshot.bets {
        lines.push(format!("bet {} {}: {} = {} {} ({})", bet.bet, bet.name, bet.metric, number(bet.value), bet.unit, bet.detail));
    }
    lines.extend(snapshot.expectations.iter().map(|outcome| outcome.line()));
    lines.extend(snapshot.gaps.iter().map(|gap| format!("gap: {gap}")));
    lines.join("\n")
}

fn number(value: Option<f64>) -> String {
    value.map_or("?".to_owned(), |value| format!("{value:.1}"))
}
