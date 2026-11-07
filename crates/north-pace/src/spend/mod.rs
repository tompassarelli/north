//! The spend rule (docs/rationale/spend-rule.md): the only reasons a new worker may not start on usage
//! grounds are a pooled reserve, a fleet-wide cap on working sessions and a burn tripwire. The
//! provider's own limit stops an account that has no room at all. Everything here is a pure function
//! of a recorded `View`; `read` gathers one from this host's state files.
mod read;
#[cfg(test)]
mod tests;
pub mod tripwire;

use std::fmt;

pub use read::{TRIPS, keyed, view_from_config};
pub use tripwire::Trip;

/// The rule's knobs, from harness/defaults.toml.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// Percent of the largest counted account's week that must stay unspent across all accounts together.
    pub reserve: f64,
    /// Points of a Max 5x week one new worker is expected to take from the pool.
    pub charge: f64,
    /// Worker sessions that may be working at once across every host and account.
    pub cap: usize,
    pub fleet_trip: tripwire::Burst,
    pub session_trip: tripwire::Burst,
    /// Hours after which an account's reading no longer counts toward the pool.
    pub reading_max_h: f64,
}

impl Rules {
    pub fn configured() -> Self {
        let number = crate::platform::harness::default_number;
        Self {
            reserve: number("spend_reserve"),
            charge: number("spend_worker_charge"),
            cap: number("spend_worker_cap") as usize,
            fleet_trip: tripwire::Burst { points: number("spend_trip_fleet_points"), minutes: number("spend_trip_fleet_min") },
            session_trip: tripwire::Burst { points: number("spend_trip_session_points"), minutes: number("spend_trip_session_min") },
            reading_max_h: number("spend_reading_max_h"),
        }
    }
}

/// One sampled window: its used percent and when it resets, in unix seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub used_pct: f64,
    pub resets_at: Option<f64>,
}

impl Window {
    /// The used percent at `at`: a window whose reset has passed has refilled.
    pub fn used(&self, at: f64) -> f64 {
        if self.resets_at.is_some_and(|resets| resets <= at) { 0.0 } else { self.used_pct }
    }
}

/// What one worker turn takes from an account's windows at p90, in percent of each window, and where the figure came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub five_hour: f64,
    pub week: f64,
    pub basis: String,
}

impl Turn {
    /// The turn priced from the ledger: the p90 API-equivalent dollars of `runs` worker runs at the account's measured points per dollar.
    pub fn measured(p90_usd: f64, runs: usize, five_hour_points_per_usd: f64, week_points_per_usd: f64) -> Self {
        Self { five_hour: p90_usd * five_hour_points_per_usd, week: p90_usd * week_points_per_usd, basis: format!("p90 ${p90_usd:.2} of {runs} worker runs") }
    }

    /// The turn assumed when the ledger gives none: `Rules::charge` Max 5x week points of the account's week.
    fn assumed(rules: &Rules, plan_week: f64) -> Self {
        Self { five_hour: 0.0, week: rules.charge / plan_week.max(f64::MIN_POSITIVE), basis: format!("assumed {:.1} week points, no measured p90", rules.charge) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    /// `provider:label`.
    pub name: String,
    /// The plan's week in Max 5x weeks, which puts every account's percent on one scale.
    pub plan_week: f64,
    pub five_hour: Option<Window>,
    pub week: Option<Window>,
    /// Whether its week counts toward the pool: a host is signed in to it and its reading is fresh.
    pub counted: bool,
    pub turn: Option<Turn>,
}

impl Account {
    /// Points of a Max 5x week the account has left before the provider stops it.
    pub fn week_left(&self, at: f64) -> f64 {
        self.week.map_or(0.0, |week| (100.0 - week.used(at)).max(0.0) * self.plan_week)
    }

    fn limit(&self, at: f64) -> Option<Refusal> {
        let (Some(five_hour), Some(week)) = (self.five_hour, self.week) else {
            return Some(Refusal { rule: Rule::Unread, line: format!("{} has no five-hour and week usage reading", self.name) });
        };
        [("five-hour", five_hour), ("week", week)].into_iter().find(|(_, window)| window.used(at) >= 100.0).map(|(kind, _)| Refusal { rule: Rule::ProviderLimit, line: format!("{} is at its provider's {kind} limit", self.name) })
    }

    /// Why one more worker turn at p90 cannot finish inside the account's five-hour and week windows.
    fn cover(&self, rules: &Rules, at: f64) -> Option<Refusal> {
        let (Some(five_hour), Some(week)) = (self.five_hour, self.week) else { return None };
        let turn = self.turn.clone().unwrap_or_else(|| Turn::assumed(rules, self.plan_week));
        [("five-hour", five_hour, turn.five_hour), ("week", week, turn.week)].into_iter().find(|(_, window, need)| window.used(at) + need >= 100.0).map(|(kind, window, need)| Refusal {
            rule: Rule::Turn,
            line: format!("{} has {:.1}% of its {kind} window left and one worker turn takes {need:.1}% at p90 ({})", self.name, (100.0 - window.used(at)).max(0.0), turn.basis),
        })
    }
}

/// What spend knows at one moment: every account's readings, the worker sessions working across the
/// fleet, and the tripped accounts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub at: f64,
    pub accounts: Vec<Account>,
    pub working: usize,
    pub trips: Vec<Trip>,
    /// Accounts that bill per token on an API key, and through the prefix their pinned `label:org` forms: no plan windows to read, so no window limit applies.
    pub keyed: Vec<String>,
}

/// What one run has already started, so it cannot place past the cap or the reserve.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placed {
    pub workers: usize,
}

/// Who starts: a worker meets every rule; an Agent-tool subagent runs inside its parent's session, so
/// it meets the reserve and the tripwire but not the cap; the proxy, a scheduled routine or one of
/// Tom's own sessions may use the reserve and counts against neither the cap nor the tripwire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Starter {
    Worker,
    Subagent,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Reserve,
    Cap,
    Tripwire,
    ProviderLimit,
    Turn,
    Unread,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reserve => "spend reserve",
            Self::Cap => "spend cap",
            Self::Tripwire => "spend tripwire",
            Self::ProviderLimit => "provider limit",
            Self::Turn => "worker turn",
            Self::Unread => "no usage reading",
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    pub rule: Rule,
    pub line: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule, self.line)
    }
}

impl View {
    /// Points of a Max 5x week left across the counted accounts.
    pub fn pool(&self) -> f64 {
        self.accounts.iter().filter(|account| account.counted).map(|account| account.week_left(self.at)).sum()
    }

    /// The reserve in Max 5x week points: its percent of the largest counted account's week.
    pub fn reserve(&self, rules: &Rules) -> f64 {
        rules.reserve * self.accounts.iter().filter(|account| account.counted).map(|account| account.plan_week).fold(0.0, f64::max)
    }

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.name == name)
    }

    /// Whether `starter` may start one more session on `account`, given what this run has placed.
    pub fn admit(&self, rules: &Rules, account: &str, starter: Starter, placed: Placed) -> Result<(), Refusal> {
        let keyed = self.keyed.iter().any(|name| account.strip_prefix(name.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with(':')));
        match starter {
            Starter::Worker if keyed => self.capped(rules, placed)?,
            Starter::Worker => self.fleet(rules, placed)?,
            Starter::Subagent if !keyed => self.reserved(rules, placed)?,
            Starter::Subagent | Starter::Reserved => {}
        }
        if keyed {
            return Ok(());
        }
        let chosen = self.account(account).ok_or_else(|| Refusal { rule: Rule::Unread, line: format!("{account} has no usage reading") })?;
        if let Some(refusal) = chosen.limit(self.at) {
            return Err(refusal);
        }
        if starter == Starter::Worker
            && let Some(refusal) = chosen.cover(rules, self.at)
        {
            return Err(refusal);
        }
        match self.trips.iter().find(|trip| trip.account == account).filter(|_| starter != Starter::Reserved) {
            Some(trip) => Err(Refusal { rule: Rule::Tripwire, line: trip.line.clone() }),
            None => Ok(()),
        }
    }

    /// The rules no account choice passes: the cap on working sessions and the pooled reserve.
    pub fn fleet(&self, rules: &Rules, placed: Placed) -> Result<(), Refusal> {
        self.capped(rules, placed)?;
        self.reserved(rules, placed)
    }

    /// The cap on working sessions alone, the one rule a worker on an API-key account meets, since it spends no subscription week.
    pub fn capped(&self, rules: &Rules, placed: Placed) -> Result<(), Refusal> {
        let working = self.working + placed.workers;
        if working >= rules.cap {
            return Err(Refusal { rule: Rule::Cap, line: format!("{working} worker sessions are working, the cap is {}", rules.cap) });
        }
        Ok(())
    }

    /// The pooled reserve alone: whether the pool, less one more charge for each placed worker and this
    /// one, stays above it.
    fn reserved(&self, rules: &Rules, placed: Placed) -> Result<(), Refusal> {
        let (after, reserve) = (self.pool() - rules.charge * (placed.workers + 1) as f64, self.reserve(rules));
        if after <= reserve {
            return Err(Refusal { rule: Rule::Reserve, line: format!("{:.1} points of week would be left across all accounts after this worker, not above the {reserve:.0}-point reserve", after.max(0.0)) });
        }
        Ok(())
    }

    /// One line for a session's grounding or `north spend`: the pool, the reserve, the cap and each trip.
    pub fn line(&self, rules: &Rules) -> String {
        let trips: Vec<&str> = self.trips.iter().map(|trip| trip.line.as_str()).collect();
        let tripped = if trips.is_empty() { String::new() } else { format!("; tripped: {}", trips.join("; ")) };
        format!("spend: {:.0} week points left across accounts, reserve {:.0}; {}/{} worker sessions working{tripped}", self.pool(), self.reserve(rules), self.working, rules.cap)
    }
}
