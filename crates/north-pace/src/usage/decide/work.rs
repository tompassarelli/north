use std::collections::BTreeSet;

use super::LEAD_FAILOVER_AFTER;
use super::place::{Decision, place};
use super::state::{State, state};
use super::world::{Label, ModelFamily, Name, Owner, Provider, Request, Session, Standing, Unix, World};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    HandOver { to: Label },
    Park { resumes_at: Option<Unix> },
}

pub fn actions(world: &World) -> Vec<(Name, Action)> {
    let mut failover = Vec::new();
    let mut rest: Vec<(bool, Name, Action)> = Vec::new();
    for session in world.sessions.iter().filter(|session| session.owner != Owner::Tom) {
        let account_state = state(world, &session.account, session.family);
        if is_failover(world, session, account_state)
            && let Some(to) = hand_over_target(world, session)
        {
            failover.push((session.name.clone(), Action::HandOver { to }));
            continue;
        }
        match account_state {
            State::Paused { resumes_at } if session.parked_since.is_none() => rest.push((session.standing == Standing::Worker, session.name.clone(), Action::Park { resumes_at })),
            State::WeekOut { .. } | State::ScopeOut { .. } => {
                if let Some(to) = hand_over_target(world, session) {
                    rest.push((session.standing == Standing::Worker, session.name.clone(), Action::HandOver { to }));
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
        .filter_map(|session| Some((provider_of(world, &session.account)?, session.family)))
        .collect();
    if in_flight.is_empty() {
        return None;
    }
    let mut earliest: Option<Unix> = None;
    for (provider, family) in in_flight {
        for account in world.accounts.iter().filter(|account| account.provider == provider) {
            earliest = match (earliest, resume_of(state(world, &account.label, family))?) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            };
        }
    }
    Some(earliest)
}

fn is_failover(world: &World, session: &Session, account_state: State) -> bool {
    let paused = matches!(account_state, State::Paused { .. });
    match session.standing {
        Standing::Proxy => paused,
        Standing::Lead => paused && session.parked_since.is_some_and(|since| world.now.0 - since.0 >= LEAD_FAILOVER_AFTER.0),
        Standing::Worker => false,
    }
}

fn hand_over_target(world: &World, session: &Session) -> Option<Label> {
    let request = Request {
        provider: provider_of(world, &session.account)?,
        family: session.family,
        owner: session.owner.clone(),
        workstream: session.workstream.clone(),
        expected: session.expected.clone(),
        pin: None,
        host: None,
    };
    match place(world, &request) {
        Decision::Place { account, .. } => Some(account),
        Decision::Refuse(_) => None,
    }
}

fn provider_of(world: &World, account: &Label) -> Option<Provider> {
    world.accounts.iter().find(|candidate| &candidate.label == account).map(|candidate| candidate.provider)
}

fn resume_of(state: State) -> Option<Option<Unix>> {
    match state {
        State::Paused { resumes_at } => Some(resumes_at),
        State::WeekOut { resets_at } | State::ScopeOut { resets_at, .. } => Some(resets_at),
        State::SignedOut | State::Unknown { .. } | State::Open { .. } => None,
    }
}
