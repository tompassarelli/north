use std::collections::BTreeSet;

use super::{LEAD_FAILOVER_AFTER, PROXY_WEEK_CEILING};
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
        let account_state = state(world, &session.account, session.family);
        if session.standing == Standing::Proxy
            && week_spent(account_state)
            && let Some(to) = week_headroom_target(world, session)
        {
            failover.push((session.name.clone(), Action::HandOver { to }));
            continue;
        }
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

fn week_spent(account_state: State) -> bool {
    matches!(account_state, State::Open { week, .. } if week >= PROXY_WEEK_CEILING)
}

fn week_headroom_target(world: &World, session: &Session) -> Option<Label> {
    roomiest(world, session, |account_state| !week_spent(account_state))
}

fn hand_over_target(world: &World, session: &Session) -> Option<Label> {
    roomiest(world, session, |_| true)
}

fn roomiest(world: &World, session: &Session, takes: impl Fn(State) -> bool) -> Option<Label> {
    let provider = provider_of(world, &session.account)?;
    let room = |account: &Account| match state(world, &account.label, session.family) {
        open @ State::Open { week, week_resets, .. } if takes(open) => Some(account.weight * (100.0 - week.0).max(0.0) / hours_to(world, week_resets)),
        _ => None,
    };
    world
        .accounts
        .iter()
        .filter(|account| account.provider == provider && account.label != session.account)
        .filter_map(|account| Some((room(account)?, &account.label)))
        .max_by(|left, right| left.0.total_cmp(&right.0).then_with(|| right.1.cmp(left.1)))
        .map(|(_, label)| label.clone())
}

fn hours_to(world: &World, at: Option<Unix>) -> f64 {
    at.map_or(WEEK_HOURS, |at| ((at.0 - world.now.0) as f64 / 3600.0).max(1.0 / 60.0))
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
