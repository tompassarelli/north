//! The spend rule (docs/rationale/spend-rule.md): the only reasons a new worker may not start on usage
//! grounds are a pooled reserve and a burn tripwire. The provider's own limit stops an account that has
//! no room at all. Everything here is a pure function of a recorded `View`; `read` gathers one from this
//! host's state files.
mod read;
#[cfg(test)]
mod tests;
pub mod drain;
pub mod operational;
pub mod tripwire;

use std::fmt;

pub use read::{TRIPS, keyed, operational_reserve, view_from_config};
pub use tripwire::Trip;

/// The rule's knobs, from harness/defaults.toml.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// Percent of the combined week of all counted accounts that must stay unspent across them together, unless the drain switch is on.
    pub reserve: f64,
    /// Percent of an account's week kept for operational callers; no new worker starts inside it.
    pub floor_week: f64,
    /// Percent of an account's five-hour window kept for operational callers; no new worker starts inside it.
    pub floor_five_hour: f64,
    /// Points of a Max 5x week one new worker is expected to take from the pool.
    pub charge: f64,
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
            floor_week: number("spend_floor_week"),
            floor_five_hour: number("spend_floor_five_hour"),
            charge: number("spend_worker_charge"),
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
    /// Accounts the policy declares an operational reserve (`[accounts.claude] reserve`): only the privacy judge, lander and canaries may use them, so no worker or subagent starts on them and their weeks stay out of the pool.
    pub operational: Vec<String>,
    /// Tom's drain switch is on: the reserve is skipped and every account may be spent to its provider limit.
    pub drain: bool,
}

/// What one run has already started, so it cannot place past the reserve.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placed {
    pub workers: usize,
}

/// Who starts: a worker meets every rule; an Agent-tool subagent runs inside its parent's session, so
/// it meets the reserve and the tripwire; the proxy, a scheduled routine or one of Tom's own sessions
/// may use the reserve and meets no tripwire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Starter {
    Worker,
    Subagent,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Reserve,
    Operational,
    Floor,
    Tripwire,
    ProviderLimit,
    Unread,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reserve => "spend reserve",
            Self::Operational => "operational reserve",
            Self::Floor => "spend floor",
            Self::Tripwire => "spend tripwire",
            Self::ProviderLimit => "provider limit",
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
    fn pooled(&self) -> impl Iterator<Item = &Account> {
        self.accounts.iter().filter(|account| account.counted && !self.operational.contains(&account.name))
    }

    /// Points of a Max 5x week left across the counted accounts, less the operational reserve.
    pub fn pool(&self) -> f64 {
        self.pooled().map(|account| account.week_left(self.at)).sum()
    }

    /// The reserve in Max 5x week points: its percent of the combined week of the counted accounts.
    pub fn reserve(&self, rules: &Rules) -> f64 {
        rules.reserve * self.pooled().map(|account| account.plan_week).sum::<f64>()
    }

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.name == name)
    }

    /// Whether `starter` may start one more session on `account`, given what this run has placed.
    pub fn admit(&self, rules: &Rules, account: &str, starter: Starter, placed: Placed) -> Result<(), Refusal> {
        if starter != Starter::Reserved && self.operational.iter().any(|name| name == account) {
            return Err(Refusal { rule: Rule::Operational, line: format!("{account} is the operational reserve: only the privacy judge, lander and canaries may use it") });
        }
        let keyed = self.keyed.iter().any(|name| account.strip_prefix(name.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with(':')));
        if starter != Starter::Reserved && !keyed {
            self.reserved(rules, placed)?;
        }
        if keyed {
            return Ok(());
        }
        let chosen = self.account(account).ok_or_else(|| Refusal { rule: Rule::Unread, line: format!("{account} has no usage reading") })?;
        if let Some(refusal) = chosen.limit(self.at) {
            return Err(refusal);
        }
        if starter == Starter::Worker {
            self.floor(rules, chosen)?;
        }
        match self.trips.iter().find(|trip| trip.account == account).filter(|_| starter != Starter::Reserved) {
            Some(trip) => Err(Refusal { rule: Rule::Tripwire, line: trip.line.clone() }),
            None => Ok(()),
        }
    }

    /// The operational floor: the last percent of each window no new worker may spend, which the
    /// privacy judge, lander and canaries keep (north#1049).
    fn floor(&self, rules: &Rules, account: &Account) -> Result<(), Refusal> {
        let kept = |window: Option<Window>, percent: f64, kind: &str| {
            let window = window.filter(|_| percent > 0.0)?;
            let left = 100.0 - window.used(self.at);
            (left <= percent).then(|| format!("{kind} {left:.0}% left, inside the {percent:.0}% floor"))
        };
        let reasons: Vec<String> = [kept(account.five_hour, rules.floor_five_hour, "five-hour"), kept(account.week, rules.floor_week, "week")].into_iter().flatten().collect();
        if reasons.is_empty() {
            return Ok(());
        }
        Err(Refusal { rule: Rule::Floor, line: format!("{} is inside its operational floor: {}; the privacy judge, lander and canaries keep it", account.name, reasons.join("; ")) })
    }

    /// The pooled reserve: whether the pool, less one more charge for each placed worker and this
    /// one, stays above it.
    fn reserved(&self, rules: &Rules, placed: Placed) -> Result<(), Refusal> {
        if self.drain {
            return Ok(());
        }
        let (after, reserve) = (self.pool() - rules.charge * (placed.workers + 1) as f64, self.reserve(rules));
        if after <= reserve {
            return Err(Refusal { rule: Rule::Reserve, line: format!("a new worker session would leave {:.1} points of week across all accounts, not above the {reserve:.0}-point reserve; `north fleet drain on` spends it", after.max(0.0)) });
        }
        Ok(())
    }

    /// One line for a session's grounding or `north spend`: the pool, the reserve, the working
    /// sessions and each trip.
    pub fn line(&self, rules: &Rules) -> String {
        let trips: Vec<&str> = self.trips.iter().map(|trip| trip.line.as_str()).collect();
        let tripped = if trips.is_empty() { String::new() } else { format!("; tripped: {}", trips.join("; ")) };
        let reserve = if self.drain { "drain on, no reserve".to_owned() } else { format!("reserve {:.0}", self.reserve(rules)) };
        format!("spend: {:.0} week points left across accounts, {reserve}; {} worker sessions working{tripped}", self.pool(), self.working)
    }
}
