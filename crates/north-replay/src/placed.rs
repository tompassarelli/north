//! Placements replayed from their recorded cases.
use std::path::Path;

use north_base::platform::decision_log;
use north_strategy_types::placement::{Case, Decision, Target};
use north_strategy_types::{Envelope, Kind};

use crate::{Deciders, parallel};
use crate::bounds::Bounds;
use crate::report::{Counts, Invariant, github_floor, stamp};

fn label(target: &Target) -> &'static str {
    match target {
        Target::Cloud { .. } => "cloud",
        Target::Account(_) => "account",
        Target::Held(_) => "held",
        Target::Unserved(_) => "unserved",
    }
}

/// The account placement targets, keyed as the usage controls are.
fn account(target: &Target) -> Option<&str> {
    match target {
        Target::Cloud { account, .. } | Target::Account(account) => Some(account),
        Target::Held(_) | Target::Unserved(_) => None,
    }
}

fn window_name(window: &str) -> Option<&'static str> {
    match window {
        "5h" => Some("five-hour"),
        other if other == "week" || other.ends_with(":week") => Some("week"),
        _ => None,
    }
}

/// Replays every placement in the window; the invariant is that the account a launch is placed on
/// keeps the floor of its five-hour and weekly windows unspent.
pub fn replay(root: &Path, since: f64, now: f64, deciders: &Deciders, bounds: &Bounds) -> (Counts, Invariant, Invariant) {
    let rule = || format!("the account a launch is placed on keeps {} points of its five-hour window and {} of its week unspent", bounds.five_hour_floor_points, bounds.week_floor_points);
    let lines = decision_log::lines(root, Kind::Placement, "jsonl", since, now);
    let parts = parallel::chunks(&lines, |part| {
        let mut counts = Counts::new("placement");
        let mut quota = Invariant::new("quota-floor", rule());
        let mut github = github_floor(bounds.github_floor_calls);
        for (_, line) in part {
            let Ok(logged) = serde_json::from_str::<Envelope<Case, Decision>>(line) else { continue };
            if logged.at < since {
                continue;
            }
            let replayed = (deciders.placement)(&logged.case);
            counts.record(label(&logged.decision.target), replayed.as_ref().map(label));
            let Some(account) = replayed.as_ref().and_then(account) else { continue };
            github.check_github(logged.case.github.as_ref(), bounds.github_floor_calls, logged.at, || format!("launch {} is placed on {account}", logged.case.launch.name));
            for window in logged.case.usage.controls.get(account).into_iter().flatten() {
                let Some((name, used)) = window.window.as_deref().and_then(window_name).zip(window.used_pct) else { continue };
                let floor = if name == "week" { bounds.week_floor_points } else { bounds.five_hour_floor_points };
                quota.check(100.0 - used >= floor, || format!("{}: launch {} is placed on {account} with {:.0} points of its {name} window left, under the floor of {floor}", stamp(logged.at), logged.case.launch.name, 100.0 - used));
            }
        }
        (counts, quota, github)
    });
    let mut counts = Counts::new("placement");
    let mut quota = Invariant::new("quota-floor", rule());
    let mut github = github_floor(bounds.github_floor_calls);
    for (part_counts, part_quota, part_github) in parts {
        counts.merge(part_counts);
        quota.merge(part_quota);
        github.merge(part_github);
    }
    (counts, quota, github)
}
