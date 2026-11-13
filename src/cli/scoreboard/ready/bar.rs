//! The bars that say the harness is ready: each bar's goal in north's concerns.toml and its label, what a reading is, and
//! whether a reading meets its target. The thresholds are the `ready` class of north's concerns.toml.
use std::collections::HashMap;
use std::fs;

use north_concerns::registry::{self, Bound, Class, Concern};

pub(super) const REPOS: [&str; 2] = ["north", "makgora"];
pub(super) const DAYS: usize = 7;
pub(super) const DAY_S: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bar {
    LandingP90,
    VerificationP50,
    VerificationP90,
    Rework,
    FirstTry,
    HarnessLoss,
    AskToLanding,
    Corrections,
    PriorityNow,
    IdleAccount,
    RuntimeBreaks,
    RuntimeRollback,
    RuntimeLive,
    RuntimeCanary,
    RuntimeShip,
    RuntimeAge,
    RuntimeLag,
    FirnClosureLive,
    UsagePerLanding,
    StrandedUsage,
    MainGreenLander,
    MainGreenCi,
}

struct Names {
    bar: Bar,
    goal: &'static str,
    label: &'static str,
}

const NAMES: [Names; 22] = [
    Names { bar: Bar::LandingP90, goal: "ready-landing-p90", label: "landing queue p90" },
    Names { bar: Bar::VerificationP50, goal: "ready-verification-p50", label: "verification p50" },
    Names { bar: Bar::VerificationP90, goal: "ready-verification-p90", label: "verification p90" },
    Names { bar: Bar::Rework, goal: "ready-rework", label: "rework in 48 h" },
    Names { bar: Bar::FirstTry, goal: "ready-first-try", label: "first-try success" },
    Names { bar: Bar::HarnessLoss, goal: "ready-harness-loss", label: "first tries lost to the harness" },
    Names { bar: Bar::AskToLanding, goal: "ready-ask-to-landing", label: "ask to landing p50" },
    Names { bar: Bar::Corrections, goal: "ready-corrections", label: "corrections per h (all)" },
    Names { bar: Bar::PriorityNow, goal: "ready-priority-now", label: "priority:now open" },
    Names { bar: Bar::IdleAccount, goal: "ready-idle-account", label: "longest idle account (all)" },
    Names { bar: Bar::RuntimeBreaks, goal: "ready-runtime-breaks", label: "runtime breaks reaching a 2nd host (all)" },
    Names { bar: Bar::RuntimeRollback, goal: "ready-runtime-rollback", label: "runtime rollback after swap, slowest (all)" },
    Names { bar: Bar::RuntimeLive, goal: "ready-runtime-live-p90", label: "promotion due to live p90, 24 h (all)" },
    Names { bar: Bar::RuntimeCanary, goal: "ready-runtime-canary-p90", label: "runtime canary p90 (all)" },
    Names { bar: Bar::RuntimeShip, goal: "ready-runtime-ship-p90", label: "runtime ship p90 (all)" },
    Names { bar: Bar::RuntimeAge, goal: "ready-runtime-age", label: "oldest landing waiting for a promotion (all)" },
    Names { bar: Bar::RuntimeLag, goal: "ready-runtime-lag", label: "runtime lag, worst process (all)" },
    Names { bar: Bar::FirnClosureLive, goal: "ready-firn-closure-live-p90", label: "firn closure landing to live p90 (all)" },
    Names { bar: Bar::UsagePerLanding, goal: "ready-usage-per-landing", label: "usage per first try landed (all)" },
    Names { bar: Bar::StrandedUsage, goal: "ready-stranded-usage", label: "usage stranded while work waited (all)" },
    Names { bar: Bar::MainGreenLander, goal: "ready-main-green-lander", label: "main green by the lander's verdicts" },
    Names { bar: Bar::MainGreenCi, goal: "ready-main-green-ci", label: "main green by GitHub Cargo" },
];

impl Bar {
    pub(super) const ALL: [Self; 22] = [
        Self::LandingP90,
        Self::VerificationP50,
        Self::VerificationP90,
        Self::Rework,
        Self::FirstTry,
        Self::HarnessLoss,
        Self::AskToLanding,
        Self::Corrections,
        Self::PriorityNow,
        Self::IdleAccount,
        Self::RuntimeBreaks,
        Self::RuntimeRollback,
        Self::RuntimeLive,
        Self::RuntimeCanary,
        Self::RuntimeShip,
        Self::RuntimeAge,
        Self::RuntimeLag,
        Self::FirnClosureLive,
        Self::UsagePerLanding,
        Self::StrandedUsage,
        Self::MainGreenLander,
        Self::MainGreenCi,
    ];

    fn names(self) -> &'static Names {
        NAMES.iter().find(|names| names.bar == self).expect("every bar has its names")
    }

    fn goal(self) -> &'static str {
        self.names().goal
    }

    pub(super) fn label(self) -> &'static str {
        self.names().label
    }

    /// Whether a window with nothing to observe still counts as passed: Tom's own activity can be absent, and a window with no rollback, no promotion that fell due, or no canary or ship has none to time.
    pub(super) fn quiet_passes(self) -> bool {
        matches!(self, Self::AskToLanding | Self::Corrections | Self::RuntimeRollback | Self::RuntimeLive | Self::RuntimeCanary | Self::RuntimeShip | Self::FirnClosureLive)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Reading {
    pub(super) value: f64,
    pub(super) sample: usize,
}

impl From<(f64, usize)> for Reading {
    fn from((value, sample): (f64, usize)) -> Self {
        Self { value, sample }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Standing {
    Pass,
    Fail,
    NoRecord,
}

impl Standing {
    pub(super) fn of(bound: Bound, reading: Option<Reading>) -> Self {
        match reading {
            Some(reading) if bound.holds(reading.value) => Self::Pass,
            Some(_) => Self::Fail,
            None => Self::NoRecord,
        }
    }
}

pub(super) fn counts(standing: Standing, bar: Bar) -> bool {
    standing == Standing::Pass || (standing == Standing::NoRecord && bar.quiet_passes())
}

pub(super) struct Goals(HashMap<&'static str, Concern>);

impl Goals {
    pub(super) fn read() -> Result<Self, String> {
        let tree = north_runtime::runtime::own_runtime().map(|(_, runtime)| runtime.join(north_runtime::runtime::layout::TREE)).filter(|tree| tree.join(registry::PATH).is_file());
        let path = tree.unwrap_or_else(|| north_concerns::code_root().join("north/main")).join(registry::PATH);
        let text = fs::read_to_string(&path).map_err(|why| format!("{}: {why}", path.display()))?;
        Self::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut goals = registry::parse(text).map_err(|found| found.join("; "))?;
        let mut held = HashMap::new();
        for bar in Bar::ALL {
            let at = goals.iter().position(|goal| goal.name == bar.goal() && goal.class == Class::Ready).ok_or_else(|| format!("no ready goal {}", bar.goal()))?;
            held.insert(bar.goal(), goals.swap_remove(at));
        }
        Ok(Self(held))
    }

    pub(super) fn of(&self, bar: Bar) -> &Concern {
        &self.0[bar.goal()]
    }
}
