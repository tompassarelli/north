use std::collections::BTreeMap;

use super::super::facts::{DAY, Facts, IssueKind, Place, Size};
use super::super::snapshot::{cpu_hours, hours_to_exhaustion, median};

const GAME_REPOS: [&str; 2] = ["smashcraft", "wisp"];
const RELEASE_REPO: &str = "smashcraft";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Metric {
    ReleaseItemsClosed,
    GameShareOfLandings,
    ReleaseShareOfLandings,
    CloudPlacements,
    IdleAccountsWhileAnotherHot,
    LaptopOverloadWhileOffloadIdle,
    LaptopShareOfCpu,
    LongestLockHoldMinutes,
    LimitStalledMinutes,
    LandToPlayableMinutes,
    MinHoursToExhaustion,
    ClosedSizeVsMedian,
    HeartbeatGapMinutes,
}

pub(crate) struct Span {
    pub from: i64,
    pub to: i64,
}

impl Metric {
    pub(crate) const ALL: [Metric; 13] = [
        Metric::ReleaseItemsClosed,
        Metric::GameShareOfLandings,
        Metric::ReleaseShareOfLandings,
        Metric::CloudPlacements,
        Metric::IdleAccountsWhileAnotherHot,
        Metric::LaptopOverloadWhileOffloadIdle,
        Metric::LaptopShareOfCpu,
        Metric::LongestLockHoldMinutes,
        Metric::LimitStalledMinutes,
        Metric::LandToPlayableMinutes,
        Metric::MinHoursToExhaustion,
        Metric::ClosedSizeVsMedian,
        Metric::HeartbeatGapMinutes,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Metric::ReleaseItemsClosed => "release_items_closed",
            Metric::GameShareOfLandings => "game_share_of_landings",
            Metric::ReleaseShareOfLandings => "release_share_of_landings",
            Metric::CloudPlacements => "cloud_placements",
            Metric::IdleAccountsWhileAnotherHot => "idle_accounts_while_another_hot",
            Metric::LaptopOverloadWhileOffloadIdle => "laptop_overload_while_offload_idle",
            Metric::LaptopShareOfCpu => "laptop_share_of_cpu",
            Metric::LongestLockHoldMinutes => "longest_lock_hold_minutes",
            Metric::LimitStalledMinutes => "limit_stalled_minutes",
            Metric::LandToPlayableMinutes => "land_to_playable_minutes",
            Metric::MinHoursToExhaustion => "min_hours_to_exhaustion",
            Metric::ClosedSizeVsMedian => "closed_size_vs_median",
            Metric::HeartbeatGapMinutes => "heartbeat_gap_minutes",
        }
    }

    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|metric| metric.name() == name)
    }

    pub(crate) fn windowed(self) -> bool {
        !matches!(
            self,
            Metric::IdleAccountsWhileAnotherHot
                | Metric::LaptopOverloadWhileOffloadIdle
                | Metric::LongestLockHoldMinutes
                | Metric::LimitStalledMinutes
                | Metric::MinHoursToExhaustion
        )
    }

    pub(crate) fn measure(self, facts: &Facts, params: &BTreeMap<String, f64>, span: &Span) -> Option<f64> {
        let param = |name: &str, default: f64| params.get(name).copied().unwrap_or(default);
        match self {
            Metric::ReleaseItemsClosed => release_items_closed(facts, span),
            Metric::GameShareOfLandings => landing_share(facts, span, |landing| GAME_REPOS.contains(&landing.repo.as_str())),
            Metric::ReleaseShareOfLandings => release_share(facts, span),
            Metric::CloudPlacements => Some(facts.cloud.as_ref()?.iter().filter(|session| within(span, session.placed_at)).count() as f64),
            Metric::IdleAccountsWhileAnotherHot => idle_accounts(facts, param("idle_below", 5.0), param("hot_above", 80.0), param("week_ceiling", 95.0)),
            Metric::LaptopOverloadWhileOffloadIdle => laptop_overload(facts, param("idle_minutes", 15.0)),
            Metric::LaptopShareOfCpu => laptop_share(facts, span),
            Metric::LongestLockHoldMinutes => Some(
                facts.locks.iter().filter(|lock| lock.holder.is_some()).map(|lock| (facts.now - lock.held_since).max(0) as f64 / 60.0).fold(0.0, f64::max),
            ),
            Metric::LimitStalledMinutes => Some(facts.limit_stops.iter().map(|stop| (facts.now - stop.resets_at).max(0) as f64 / 60.0).fold(0.0, f64::max)),
            Metric::LandToPlayableMinutes => {
                facts.builds.iter().filter(|build| within(span, build.at)).map(|build| build.land_to_playable_seconds / 60.0).reduce(f64::max)
            }
            Metric::MinHoursToExhaustion => min_hours_to_exhaustion(facts),
            Metric::ClosedSizeVsMedian => closed_size_vs_median(facts, span, param("history_days", 7.0) as i64),
            Metric::HeartbeatGapMinutes => heartbeat_gap(facts, span),
        }
    }
}

fn within(span: &Span, at: i64) -> bool {
    at >= span.from && at < span.to
}

fn release_items_closed(facts: &Facts, span: &Span) -> Option<f64> {
    let issues = facts.release.as_ref()?;
    Some(issues.iter().filter(|issue| issue.kind == IssueKind::Work && issue.closed_at.is_some_and(|at| within(span, at))).count() as f64)
}

fn landing_share(facts: &Facts, span: &Span, counts: impl Fn(&super::super::facts::Landing) -> bool) -> Option<f64> {
    let landings: Vec<_> = facts.landings.iter().filter(|landing| within(span, landing.at)).collect();
    (!landings.is_empty()).then(|| landings.iter().filter(|landing| counts(landing)).count() as f64 / landings.len() as f64)
}

fn release_share(facts: &Facts, span: &Span) -> Option<f64> {
    let numbers: Vec<u32> = facts.release.as_ref()?.iter().filter(|issue| issue.kind == IssueKind::Work).map(|issue| issue.number).collect();
    landing_share(facts, span, |landing| landing.repo == RELEASE_REPO && landing.issue_refs.iter().any(|number| numbers.contains(number)))
}

fn claude_windows(facts: &Facts) -> Vec<(Option<f64>, Option<f64>)> {
    let Some(table) = facts.pace.as_ref().and_then(|pace| pace["accounts"].as_object()) else { return Vec::new() };
    table
        .iter()
        .filter(|(name, _)| name.starts_with("claude:"))
        .map(|(_, entry)| {
            let used = |label: &str| {
                entry["windows"].as_array().into_iter().flatten().find(|window| window["window"] == label).and_then(|window| window["used_pct"].as_f64())
            };
            (used("5h"), used("week"))
        })
        .collect()
}

fn idle_accounts(facts: &Facts, idle_below: f64, hot_above: f64, week_ceiling: f64) -> Option<f64> {
    let accounts = claude_windows(facts);
    if accounts.is_empty() {
        return None;
    }
    let usable: Vec<f64> = accounts.iter().filter(|(_, week)| week.is_none_or(|week| week < week_ceiling)).filter_map(|(five, _)| *five).collect();
    let hot = usable.iter().any(|five| *five > hot_above);
    Some(if hot { usable.iter().filter(|five| **five < idle_below).count() as f64 } else { 0.0 })
}

fn laptop_overload(facts: &Facts, idle_minutes: f64) -> Option<f64> {
    let load = facts.load?;
    let cores = f64::from(load.cores);
    let recent = facts.now - (idle_minutes * 60.0) as i64;
    let nexus_busy = facts.runs.iter().any(|run| run.place == Place::Nexus && run.at >= recent);
    let farm_busy = facts.farm.iter().flatten().any(|run| run.created_at >= recent);
    let cloud_busy = facts.cloud.iter().flatten().any(|session| session.status == "running");
    let overloaded = load.five > cores && load.fifteen > cores;
    Some(f64::from(u8::from(overloaded && !nexus_busy && !farm_busy && !cloud_busy)))
}

fn laptop_share(facts: &Facts, span: &Span) -> Option<f64> {
    let laptop = cpu_hours(facts, Place::Laptop, span.from, span.to)?;
    let nexus = cpu_hours(facts, Place::Nexus, span.from, span.to)?;
    let farm = cpu_hours(facts, Place::HostedFarm, span.from, span.to)?;
    let total = laptop + nexus + farm;
    (total > 0.0).then(|| laptop / total)
}

fn min_hours_to_exhaustion(facts: &Facts) -> Option<f64> {
    let table = facts.pace.as_ref()?["accounts"].as_object()?;
    table
        .iter()
        .filter(|(name, _)| name.starts_with("claude:"))
        .flat_map(|(_, entry)| entry["windows"].as_array().into_iter().flatten())
        .filter_map(|window| hours_to_exhaustion(window, facts.now))
        .reduce(f64::min)
}

fn closed_points(facts: &Facts, from: i64, to: i64) -> Option<f64> {
    let release = facts.release.as_ref()?.iter().filter(|issue| issue.kind == IssueKind::Work).map(|issue| (issue.closed_at, issue.size));
    let bets = facts.bet_issues.iter().flatten().map(|issue| (issue.closed_at, issue.size));
    Some(release.chain(bets).filter(|(closed, _)| closed.is_some_and(|at| at >= from && at < to)).map(|(_, size)| size.unwrap_or(Size::M).points()).sum())
}

fn closed_size_vs_median(facts: &Facts, span: &Span, history_days: i64) -> Option<f64> {
    let today = closed_points(facts, span.to - DAY, span.to)?;
    let mut history: Vec<f64> =
        (1..=history_days).filter_map(|back| closed_points(facts, span.to - (back + 1) * DAY, span.to - back * DAY)).collect();
    let median = median(&mut history).filter(|median| *median > 0.0)?;
    Some(today / median)
}

fn heartbeat_gap(facts: &Facts, span: &Span) -> Option<f64> {
    let mut beats: Vec<i64> = facts.checks.iter().copied().filter(|at| within(span, *at)).collect();
    beats.sort_unstable();
    let last = *beats.last()?;
    let end = span.to.min(facts.now);
    let gaps = beats.windows(2).map(|pair| pair[1] - pair[0]).chain([end - last]);
    gaps.max().map(|gap| gap.max(0) as f64 / 60.0)
}
