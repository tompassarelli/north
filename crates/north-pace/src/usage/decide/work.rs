use std::collections::BTreeSet;

use super::keep::{Footing, footing, keep, strand};
use super::state::{State, state};
use super::world::{Account, Label, ModelFamily, Name, Owner, Provider, Session, Standing, Unix, World};

const WEEK_HOURS: f64 = 7.0 * 24.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    HandOver { to: Label },
    Park { resumes_at: Option<Unix> },
}

pub fn actions(world: &World) -> Vec<(Name, Action)> {
    let mut failover = Vec::new();
    let mut rest: Vec<(bool, Name, Action)> = Vec::new();
    for session in world.sessions.iter().filter(|session| session.owner != Owner::Tom) {
        if session.standing != Standing::Worker {
            if let Some(action) = keep(world, session) {
                failover.push((session.name.clone(), action));
            }
            continue;
        }
        if let Some(action) = strand(world, session) {
            rest.push((true, session.name.clone(), action));
            continue;
        }
        match state(world, &session.account, session.family) {
            State::Paused { resumes_at } if session.parked_since.is_none() => rest.push((true, session.name.clone(), Action::Park { resumes_at })),
            State::WeekOut { .. } | State::ScopeOut { .. } => {
                if let Some(to) = soonest_expiring(world, session) {
                    rest.push((true, session.name.clone(), Action::HandOver { to }));
                }
            }
            _ => {}
        }
    }
    failover.sort_by(|left, right| left.0.cmp(&right.0));
    rest.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
    failover.extend(rest.into_iter().map(|(_, name, action)| (name, action)));
    failover
}

pub fn all_out(world: &World) -> Option<Option<Unix>> {
    let in_flight: BTreeSet<(Provider, ModelFamily)> = world
        .sessions
        .iter()
        .filter(|session| session.owner != Owner::Tom)
        .filter_map(|session| Some((world.accounts.iter().find(|account| account.label == session.account)?.provider, session.family)))
        .collect();
    if in_flight.is_empty() {
        return None;
    }
    let mut earliest: Option<Unix> = None;
    for (provider, family) in in_flight {
        for account in world.accounts.iter().filter(|account| account.provider == provider) {
            let resumes = match (account.credit, footing(world, account, family)) {
                (Some(_), Footing::Below { windfall }) => windfall,
                (Some(_), Footing::Unknown) => continue,
                (Some(_), Footing::Above { .. }) => return None,
                (None, _) => resume_of(state(world, &account.label, family))?,
            };
            earliest = match (earliest, resumes) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            };
        }
    }
    Some(earliest)
}

fn resume_of(state: State) -> Option<Option<Unix>> {
    match state {
        State::Paused { resumes_at } => Some(resumes_at),
        State::WeekOut { resets_at } | State::ScopeOut { resets_at, .. } => Some(resets_at),
        State::SignedOut | State::Unknown { .. } | State::Open { .. } => None,
    }
}

fn soonest_expiring(world: &World, session: &Session) -> Option<Label> {
    let provider = world.accounts.iter().find(|account| account.label == session.account)?.provider;
    let room = |account: &Account| match state(world, &account.label, session.family) {
        State::Open { week, week_resets, .. } => Some(account.weight * (100.0 - week.0).max(0.0) / hours_to(world, week_resets)),
        _ => None,
    };
    world
        .accounts
        .iter()
        .filter(|account| account.provider == provider && account.label != session.account && account.credit.is_none())
        .filter_map(|account| Some((room(account)?, &account.label)))
        .max_by(|left, right| left.0.total_cmp(&right.0).then_with(|| right.1.cmp(left.1)))
        .map(|(_, label)| label.clone())
}

fn hours_to(world: &World, at: Option<Unix>) -> f64 {
    at.map_or(WEEK_HOURS, |at| ((at.0 - world.now.0) as f64 / 3600.0).max(1.0 / 60.0))
}
