//! What north knows of account spend (docs/rationale/spend-rule.md): each account's sampled windows, the week
//! left across them, and the three reasons a session may not start on an account: the owner holds every API
//! organization, the provider has stopped it, or the policy declares it the operational reserve. Everything here is a pure function of a recorded `View`;
//! `read` gathers one from this host's state files.
mod read;
#[cfg(test)]
mod tests;
pub mod hold;
pub mod operational;

use std::fmt;

pub use read::{operational_reserve, view_from_config};

/// The view's settings, from harness/defaults.toml.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// Points of a Max 5x week one new worker is expected to take, which dispatch charges an account so its next box ranks the others first.
    pub charge: f64,
    /// Hours after which an account's reading no longer counts toward the pool.
    pub reading_max_h: f64,
}

impl Rules {
    pub fn configured() -> Self {
        let number = crate::platform::harness::default_number;
        Self { charge: number("spend_worker_charge"), reading_max_h: number("spend_reading_max_h") }
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
    /// Accounts the policy declares an operational reserve (`[accounts.claude] reserve`): only the privacy judge, lander and canaries may use them, so no worker or subagent starts on them and their weeks stay out of the pool.
    pub operational: Vec<String>,
    /// The owner's hold on every API organization (`[accounts.claude] api_hold`), which refuses an `api` account to every starter.
    pub held: Option<hold::Hold>,
}

/// Who starts: a worker or an Agent-tool subagent, or a reserved caller (the proxy, a scheduled routine, one of
/// Tom's own sessions, the privacy judge, lander and canaries), which alone may use the operational reserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Starter {
    Worker,
    Reserved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Held,
    Operational,
    ProviderLimit,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Held => "api hold",
            Self::Operational => "operational reserve",
            Self::ProviderLimit => "provider limit",
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
    /// Points of a Max 5x week left across the counted accounts, less the operational reserve.
    pub fn pool(&self) -> f64 {
        self.accounts.iter().filter(|account| account.counted && !self.operational.contains(&account.name)).map(|account| account.week_left(self.at)).sum()
    }

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.name == name)
    }

    /// Whether `starter` may start one more session on `account`: only the owner's API hold, the declared operational reserve and the provider's own limit refuse.
    pub fn admit(&self, account: &str, starter: Starter) -> Result<(), Refusal> {
        if let Some(line) = self.held.as_ref().and_then(|hold| hold.refusal(account)) {
            return Err(Refusal { rule: Rule::Held, line });
        }
        if starter != Starter::Reserved && self.operational.iter().any(|name| name == account) {
            return Err(Refusal { rule: Rule::Operational, line: format!("{account} is the operational reserve: only the privacy judge, lander and canaries may use it") });
        }
        match self.account(account).and_then(|chosen| chosen.limit(self.at)) {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }

    /// One line for a session's grounding or `north spend`: the pool and the working sessions.
    pub fn line(&self) -> String {
        format!("spend: {:.0} week points left across accounts; {} worker sessions working", self.pool(), self.working)
    }
}
