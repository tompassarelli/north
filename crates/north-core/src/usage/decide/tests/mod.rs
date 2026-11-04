mod properties;
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

#[derive(Debug, Clone, Default)]
pub struct Fixture {
    pub accounts: Vec<Account>,
    pub readings: Vec<Reading>,
    pub limits: Vec<LimitEvent>,
    pub settings: Settings,
}

impl Fixture {
    pub fn signed_in(name: &str) -> Self {
        Self { accounts: vec![account(name, &["gex45"])], ..Self::default() }
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
        World { now: NOW, accounts: &self.accounts, readings: &self.readings, limits: &self.limits, sessions: &[], credit: None, settings: &self.settings }
    }
}
