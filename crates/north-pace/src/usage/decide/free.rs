use super::view::{View, view};
use super::world::{Account, Label, Owner, Provider, Request, Session, Window, World};

#[derive(Debug, Clone, PartialEq)]
pub struct Free {
    pub percent: f64,
    pub in_flight: f64,
    pub unread: Vec<Label>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shortfall {
    pub free: f64,
    pub in_flight: f64,
    pub request: f64,
    pub reserve: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gate {
    Pass { crossed: bool },
    Reserve(Shortfall),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exemption {
    Owner,
    NoReserve,
    Crossed,
}

pub fn free(world: &World, provider: Provider) -> Free {
    let total = total_weight(world, provider);
    if total <= 0.0 {
        return Free { percent: 0.0, in_flight: 0.0, unread: Vec::new() };
    }
    let mut percent = 0.0;
    let mut unread = Vec::new();
    for account in provider_accounts(world, provider) {
        match view(world, &account.label, Window::Week) {
            View::Fresh { used, .. } => percent += account.weight * (100.0 - used.0).max(0.0),
            View::NeverRead | View::Stale { .. } => unread.push(account.label.clone()),
            View::Limited { .. } => {}
        }
    }
    unread.sort();
    Free { percent: percent / total, in_flight: in_flight(world, provider, total), unread }
}

pub fn gate(world: &World, request: &Request, account: &Account, free: &Free) -> Gate {
    match exemption(world, request) {
        Some(Exemption::Crossed) => return Gate::Pass { crossed: true },
        Some(Exemption::Owner | Exemption::NoReserve) => return Gate::Pass { crossed: false },
        None => {}
    }
    let reserve = reserve(world, request.provider);
    let total = total_weight(world, request.provider);
    let this_request = account.weight * account.rates.week.points_per_usd() * request.expected.p50.0 / total;
    if free.percent - free.in_flight - this_request >= reserve {
        Gate::Pass { crossed: false }
    } else {
        Gate::Reserve(Shortfall { free: free.percent, in_flight: free.in_flight, request: this_request, reserve })
    }
}

pub fn reserve_applies(world: &World, request: &Request) -> bool {
    exemption(world, request).is_none()
}

pub fn remaining(session: &Session) -> f64 {
    (session.expected.p50.0 - session.spent.map_or(0.0, |spent| spent.0)).max(0.0)
}

pub fn provider_accounts<'a>(world: &'a World, provider: Provider) -> impl Iterator<Item = &'a Account> {
    world.accounts.iter().filter(move |account| account.provider == provider)
}

fn exemption(world: &World, request: &Request) -> Option<Exemption> {
    if request.owner == Owner::Tom {
        return Some(Exemption::Owner);
    }
    if reserve(world, request.provider) <= 0.0 {
        return Some(Exemption::NoReserve);
    }
    let crossed = world.settings.crossings.iter().any(|crossing| crossing.provider == request.provider && crossing.until > world.now);
    crossed.then_some(Exemption::Crossed)
}

fn reserve(world: &World, provider: Provider) -> f64 {
    world.settings.reserve.get(&provider).map_or(0.0, |percent| percent.0)
}

fn total_weight(world: &World, provider: Provider) -> f64 {
    provider_accounts(world, provider).map(|account| account.weight).sum()
}

fn in_flight(world: &World, provider: Provider, total: f64) -> f64 {
    let unparked = world.sessions.iter().filter(|session| session.parked_since.is_none() && session.owner != Owner::Tom);
    unparked
        .filter_map(|session| {
            let account = provider_accounts(world, provider).find(|account| account.label == session.account)?;
            Some(account.weight * account.rates.week.points_per_usd() * remaining(session) / total)
        })
        .sum()
}
