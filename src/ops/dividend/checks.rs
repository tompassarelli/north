//! The four first dividends, each a pure verdict over facts read once: the metric over the last
//! seven days against its target.
use std::fmt;

pub(crate) const WINDOW_DAYS: f64 = 7.0;
pub(crate) const DAY_S: f64 = 86_400.0;
pub(crate) const LAPTOP_SHARE_BELOW: f64 = 0.8;
pub(crate) const GAME_SHARE_AT_LEAST: f64 = 0.6;
pub(crate) const RELEASE_CLOSED_PER_DAY: f64 = 1.0;
const GAME_REPOS: [&str; 2] = ["smashcraft", "wisp"];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Check {
    CloudLane,
    LaptopShare,
    GameShare,
    ReleaseClosed,
}

impl Check {
    pub(crate) const ALL: [Check; 4] = [
        Check::CloudLane,
        Check::LaptopShare,
        Check::GameShare,
        Check::ReleaseClosed,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Check::CloudLane => "cloud-lane",
            Check::LaptopShare => "laptop-share",
            Check::GameShare => "game-share",
            Check::ReleaseClosed => "release-closed",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Check> {
        Check::ALL.into_iter().find(|check| check.name() == name)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Standing {
    Paying,
    Decayed,
    Unread,
}

impl fmt::Display for Standing {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Standing::Paying => "PAYING",
            Standing::Decayed => "DECAYED",
            Standing::Unread => "UNREAD",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Verdict {
    pub standing: Standing,
    pub line: String,
}

/// Measured CPU seconds of the last seven days: the laptop's, all hosts', and the hosts whose log
/// could not be read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cpu {
    pub laptop: f64,
    pub total: f64,
    pub unmeasured: Vec<String>,
}

/// What the checks read, once per run. A read that failed is its reason, which makes its checks
/// UNREAD rather than paying or decayed.
#[derive(Clone, Debug)]
pub(crate) struct Facts {
    pub at: f64,
    pub cloud_launches: Vec<f64>,
    pub credit: (bool, String),
    pub cpu: Result<Cpu, String>,
    pub landings: Result<Vec<(f64, String)>, String>,
    pub release_closed: Result<Vec<f64>, String>,
}

pub(crate) fn evaluate(check: Check, facts: &Facts) -> Verdict {
    match check {
        Check::CloudLane => cloud_lane(facts),
        Check::LaptopShare => laptop_share(facts),
        Check::GameShare => game_share(facts),
        Check::ReleaseClosed => release_closed(facts),
    }
}

fn verdict(paying: bool, line: String) -> Verdict {
    Verdict {
        standing: if paying {
            Standing::Paying
        } else {
            Standing::Decayed
        },
        line,
    }
}

fn unread(reason: &str) -> Verdict {
    Verdict {
        standing: Standing::Unread,
        line: format!("unread: {reason}"),
    }
}

fn within(times: &[f64], at: f64, days: f64) -> usize {
    times
        .iter()
        .filter(|time| at - **time < days * DAY_S && **time <= at)
        .count()
}

fn cloud_lane(facts: &Facts) -> Verdict {
    let (live, credit) = &facts.credit;
    if !live {
        return Verdict {
            standing: Standing::Paying,
            line: format!("retired, the cloud credit is not live ({credit})"),
        };
    }
    let (day, week) = (
        within(&facts.cloud_launches, facts.at, 1.0),
        within(&facts.cloud_launches, facts.at, WINDOW_DAYS),
    );
    verdict(
        day > 0,
        format!(
            "{day} code-only boxes placed in the cloud in the last day, {week} in 7 days; target more than 0 a day while the credit lasts"
        ),
    )
}

fn laptop_share(facts: &Facts) -> Verdict {
    let cpu = match &facts.cpu {
        Ok(cpu) => cpu,
        Err(reason) => return unread(reason),
    };
    if cpu.total <= 0.0 {
        return unread("no CPU-hours measured in 7 days");
    }
    let share = cpu.laptop / cpu.total;
    let skipped = if cpu.unmeasured.is_empty() {
        String::new()
    } else {
        format!(" (unmeasured: {})", cpu.unmeasured.join(", "))
    };
    verdict(
        share < LAPTOP_SHARE_BELOW,
        format!(
            "laptop ran {:.0}% of {:.1} measured CPU-hours in 7 days{skipped}; target under {:.0}%",
            share * 100.0,
            cpu.total / 3600.0,
            LAPTOP_SHARE_BELOW * 100.0
        ),
    )
}

fn game_share(facts: &Facts) -> Verdict {
    let landings = match &facts.landings {
        Ok(landings) => landings,
        Err(reason) => return unread(reason),
    };
    let recent: Vec<&str> = landings
        .iter()
        .filter(|(time, _)| facts.at - time < WINDOW_DAYS * DAY_S && *time <= facts.at)
        .map(|(_, repo)| repo.as_str())
        .collect();
    if recent.is_empty() {
        return verdict(false, "no landings in 7 days".into());
    }
    let game = recent
        .iter()
        .filter(|repo| GAME_REPOS.contains(repo))
        .count();
    let share = game as f64 / recent.len() as f64;
    verdict(
        share >= GAME_SHARE_AT_LEAST,
        format!(
            "{game} of {} landings in 7 days were game work ({:.0}%); target at least {:.0}%",
            recent.len(),
            share * 100.0,
            GAME_SHARE_AT_LEAST * 100.0
        ),
    )
}

fn release_closed(facts: &Facts) -> Verdict {
    let closed = match &facts.release_closed {
        Ok(closed) => closed,
        Err(reason) => return unread(reason),
    };
    let week = within(closed, facts.at, WINDOW_DAYS);
    let per_day = week as f64 / WINDOW_DAYS;
    verdict(
        per_day >= RELEASE_CLOSED_PER_DAY,
        format!(
            "{week} release items closed in 7 days, {per_day:.1} a day; target at least {RELEASE_CLOSED_PER_DAY:.0} a day"
        ),
    )
}
