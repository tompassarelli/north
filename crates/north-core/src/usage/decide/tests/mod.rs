mod free;
mod place;
mod place_properties;
mod properties;
mod rank;
mod state;
mod view;

use std::collections::BTreeSet;

use super::world::*;

pub const NOW: Unix = Unix(1_000_000);

pub fn label(name: &str) -> Label {
    Label(name.to_owned())
}

pub fn account(name: &str, hosts: &[&str]) -> Account {
    let rate = Rate::Pooled { points_per_usd: 1.0, n: 0 };
    Account {
        provider: Provider::Claude,
        label: label(name),
        weight: 1.0,
        signed_in: hosts.iter().map(|host| Host((*host).to_owned())).collect::<BTreeSet<_>>(),
        extra_usage_enabled: Some(false),
        extra_usage_spend: None,
        rates: Rates { five_hour: rate, week: rate },
    }
}

pub fn reading(row: i64, name: &str, window: Window, used: f64, resets_at: Option<i64>, read_at: i64, source: Source) -> Reading {
    Reading { row: RowId(row), account: label(name), window, used: Percent(used), resets_at: resets_at.map(Unix), read_at: Unix(read_at), source }
}

pub fn fresh(row: i64, name: &str, window: Window, used: f64, resets_at: Option<i64>) -> Reading {
    reading(row, name, window, used, resets_at, NOW.0 - 60, Source::Header)
}

pub fn limit(name: &str, window: Window, resets_at: Option<i64>, at: i64) -> LimitEvent {
    LimitEvent { account: label(name), window, resets_at: resets_at.map(Unix), at: Unix(at) }
}

pub fn request() -> Request {
    Request { provider: Provider::Claude, family: ModelFamily::Opus, owner: Owner::Fleet, workstream: None, expected: expected(0.0), pin: None, host: None }
}

pub fn expected(p50: f64) -> Expected {
    Expected { p50: Usd(p50), p90: Usd(p50 * 2.0), minutes: 30.0, source: EstimateSource::None }
}

pub fn session(name: &str, account: &str, p50: f64, spent: Option<f64>) -> Session {
    Session {
        name: Name(name.to_owned()),
        owner: Owner::Fleet,
        account: label(account),
        family: ModelFamily::Opus,
        workstream: None,
        box_id: None,
        lead: false,
        spent: spent.map(Usd),
        active_minutes: 0.0,
        expected: expected(p50),
        parked_since: None,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Fixture {
    pub accounts: Vec<Account>,
    pub readings: Vec<Reading>,
    pub limits: Vec<LimitEvent>,
    pub sessions: Vec<Session>,
    pub tripped: Vec<Trip>,
    pub settings: Settings,
}

impl Fixture {
    pub fn signed_in(name: &str) -> Self {
        Self { accounts: vec![account(name, &["gex45"])], ..Self::default() }
    }

    pub fn accounts(names: &[&str]) -> Self {
        Self { accounts: names.iter().map(|name| account(name, &["gex45"])).collect(), ..Self::default() }
    }

    pub fn open(self, name: &str, week: f64, week_resets: i64, five_hour: f64, five_hour_resets: i64) -> Self {
        let base = self.readings.len() as i64 * 2;
        self.with(fresh(base, name, Window::Week, week, Some(week_resets))).with(fresh(base + 1, name, Window::FiveHour, five_hour, Some(five_hour_resets)))
    }

    pub fn running(mut self, session: Session) -> Self {
        self.sessions.push(session);
        self
    }

    pub fn with(mut self, reading: Reading) -> Self {
        self.readings.push(reading);
        self
    }

    pub fn limited(mut self, event: LimitEvent) -> Self {
        self.limits.push(event);
        self
    }

    pub fn world(&self) -> World<'_> {
        World {
            now: NOW,
            accounts: &self.accounts,
            readings: &self.readings,
            limits: &self.limits,
            sessions: &self.sessions,
            tripped: &self.tripped,
            credit: None,
            settings: &self.settings,
        }
    }
}
