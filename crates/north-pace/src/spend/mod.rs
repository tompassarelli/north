//! The spend rule (docs/rationale/spend-rule.md): the only reasons a new worker may not start on usage
//! grounds are a pooled reserve, an operational floor and a burn governor. The provider's own limit stops an account that has
//! no room at all. Everything here is a pure function of a recorded `View`; `read` gathers one from this
//! host's state files.
pub mod burn;
mod read;
mod starting;
#[cfg(test)]
mod tests;
pub mod drain;
pub mod operational;

use std::fmt;

pub use read::{BURNS, keyed, operational_reserve, view_from_config};
pub use burn::Burn;

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
    /// Minutes over which the fleet's rise in an account's five-hour window is measured for the burn governor.
    pub five_hour_burn_min: f64,
    /// Hours over which the fleet's rise in an account's week is measured for the burn governor.
    pub week_burn_h: f64,
    /// Hours after which an account's reading no longer counts toward the pool.
    pub reading_max_h: f64,
    /// Urgent sessions per account that may run past the burn governor at once.
    pub urgent_limit: usize,
}

impl Rules {
    pub fn configured() -> Self {
        let number = crate::platform::harness::default_number;
        Self {
            reserve: number("spend_reserve"),
            floor_week: number("spend_floor_week"),
            floor_five_hour: number("spend_floor_five_hour"),
            charge: number("spend_worker_charge"),
            five_hour_burn_min: number("spend_burn_five_hour_min"),
            week_burn_h: number("spend_burn_week_h"),
            reading_max_h: number("spend_reading_max_h"),
            urgent_limit: number("spend_urgent_limit") as usize,
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
        let full = |window: Option<Window>| window.is_some_and(|window| window.used(at) >= 100.0);
        [("five-hour", self.five_hour), ("week", self.week)].into_iter().find(|(_, window)| full(*window)).map(|(kind, _)| Refusal { rule: Rule::ProviderLimit, line: format!("{} is at its provider's {kind} limit", self.name) })
    }
}

/// What spend knows at one moment: every account's readings and the worker sessions working across the
/// fleet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub at: f64,
    pub accounts: Vec<Account>,
    pub working: usize,
    /// The fleet's measured burn on each account, which the burn governor projects to the account's resets.
    pub burns: Vec<Burn>,
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
/// it meets the reserve; the proxy, a scheduled routine or one of Tom's own sessions
/// may use the reserve. An urgent worker (a spine fix launched `--priority urgent`)
/// meets every rule a worker does but the burn governor while fewer than `Rules::urgent_limit` urgent
/// sessions already run on its account, `running` being that count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Starter {
    Worker,
    Urgent { running: usize },
    Subagent,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Reserve,
    Operational,
    Floor,
    Burn,
    ProviderLimit,
    Unread,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reserve => "spend reserve",
            Self::Operational => "operational reserve",
            Self::Floor => "spend floor",
            Self::Burn => "burn governor",
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

    /// Whether `starter` may start one more session on `account`: only the declared operational reserve and the provider's own limit refuse.
    pub fn admit(&self, _rules: &Rules, account: &str, starter: Starter, _placed: Placed) -> Result<(), Refusal> {
        if starter != Starter::Reserved && self.operational.iter().any(|name| name == account) {
            return Err(Refusal { rule: Rule::Operational, line: format!("{account} is the operational reserve: only the privacy judge, lander and canaries may use it") });
        }
        match self.account(account).and_then(|chosen| chosen.limit(self.at)) {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }

    /// One line for a session's grounding or `north spend`: the pool, the reserve and the working
    /// sessions.
    pub fn line(&self, rules: &Rules) -> String {
        let reserve = if self.drain { "drain on, no reserve".to_owned() } else { format!("reserve {:.0}", self.reserve(rules)) };
        format!("spend: {:.0} week points left across accounts, {reserve}; {} worker sessions working", self.pool(), self.working)
    }
}
