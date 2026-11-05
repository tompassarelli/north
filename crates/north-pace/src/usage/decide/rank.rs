use super::free::{Free, Gate, Shortfall, free, gate, provider_accounts, remaining, reserve_applies};
use super::state::{State, state};
use super::world::{Account, Label, Request, Unix, World};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    One,
    Two,
    Three,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Warning {
    MayPause { finish_by: Unix, resumes_at: Option<Unix> },
    Unread { read_at: Option<Unix> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Block {
    Closed(State),
    Unread { read_at: Option<Unix> },
    ExtraUsage,
    Reserve(Shortfall),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Placeable { tier: Tier, warning: Option<Warning>, gate: Gate },
    Blocked(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub account: Label,
    pub state: State,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub entries: Vec<Entry>,
    pub free: Free,
}

impl Ranked {
    pub fn placeable(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| matches!(entry.verdict, Verdict::Placeable { .. }))
    }

    pub fn entry(&self, account: &Label) -> Option<&Entry> {
        self.entries.iter().find(|entry| &entry.account == account)
    }
}

pub fn rank(world: &World, request: &Request) -> Ranked {
    let free = free(world, request.provider);
    let mut placeable = Vec::new();
    let mut blocked = Vec::new();
    for account in provider_accounts(world, request.provider) {
        let account_state = state(world, &account.label, request.family);
        let verdict = verdict(world, request, account, account_state, &free);
        let entry = Entry { account: account.label.clone(), state: account_state, verdict };
        match verdict {
            Verdict::Placeable { tier, .. } => placeable.push((order_key(tier, account_state, &account.label), entry)),
            Verdict::Blocked(_) => blocked.push(entry),
        }
    }
    placeable.sort_by(|left, right| left.0.cmp(&right.0));
    blocked.sort_by(|left, right| left.account.cmp(&right.account));
    let entries = placeable.into_iter().map(|(_, entry)| entry).chain(blocked).collect();
    Ranked { entries, free }
}

fn verdict(world: &World, request: &Request, account: &Account, account_state: State, free: &Free) -> Verdict {
    match account_state {
        State::SignedOut | State::WeekOut { .. } | State::ScopeOut { .. } | State::Paused { .. } => Verdict::Blocked(Block::Closed(account_state)),
        State::Unknown { read_at, .. } => unread(world, request, account, read_at),
        State::Open { five_hour, five_hour_resets, .. } => {
            let gate = gate(world, request, account, free);
            if let Gate::Reserve(shortfall) = gate {
                return Verdict::Blocked(Block::Reserve(shortfall));
            }
            if fits(world, request, account, five_hour.0) {
                return Verdict::Placeable { tier: Tier::One, warning: None, gate };
            }
            if extra_usage_enabled(account) {
                return Verdict::Blocked(Block::ExtraUsage);
            }
            let finish_by = Unix(world.now.0 + (request.expected.minutes * 60.0).round() as i64);
            Verdict::Placeable { tier: Tier::Two, warning: Some(Warning::MayPause { finish_by, resumes_at: five_hour_resets }), gate }
        }
    }
}

fn unread(world: &World, request: &Request, account: &Account, read_at: Option<Unix>) -> Verdict {
    if reserve_applies(world, request) {
        return Verdict::Blocked(Block::Unread { read_at });
    }
    if extra_usage_enabled(account) {
        return Verdict::Blocked(Block::ExtraUsage);
    }
    Verdict::Placeable { tier: Tier::Three, warning: Some(Warning::Unread { read_at }), gate: Gate::Pass { crossed: false } }
}

fn fits(world: &World, request: &Request, account: &Account, five_hour_used: f64) -> bool {
    let in_flight: f64 = world.sessions.iter().filter(|session| session.parked_since.is_none() && session.account == account.label).map(remaining).sum();
    (100.0 - five_hour_used) - account.rates.five_hour.points_per_usd() * (in_flight + request.expected.p50.0) >= 0.0
}

fn extra_usage_enabled(account: &Account) -> bool {
    account.extra_usage_enabled == Some(true)
}

fn order_key(tier: Tier, account_state: State, label: &Label) -> (Tier, i64, Label) {
    let reset = match (tier, account_state) {
        (Tier::Two, State::Open { five_hour_resets, .. }) => five_hour_resets,
        (_, State::Open { week_resets, .. }) => week_resets,
        _ => None,
    };
    (tier, reset.map_or(i64::MAX, |reset| reset.0), label.clone())
}
