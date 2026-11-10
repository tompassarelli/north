//! Continuity before spending: a proxy or lead leaves its source before the source runs out, for the
//! source above the floor that keeps it running longest, and with none above the floor waits for the
//! soonest windfall. A worker whose source pauses moves to that same source above the floor, and parks only when none is.
use super::state::{State, state};
use super::work::Action;
use super::world::{Account, Credit, Label, ModelFamily, Percent, Session, Unix, Usd, World};

pub const SUBSCRIPTION_FLOOR: Percent = Percent(98.0);
pub const API_FLOOR: Usd = Usd(50.0);
const RENEWS: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Footing {
    Above { worth: f64 },
    Below { windfall: Option<Unix> },
    Unknown,
}

pub(super) fn footing(world: &World, account: &Account, family: ModelFamily) -> Footing {
    if account.signed_in.is_empty() {
        return Footing::Unknown;
    }
    if let Some(credit) = account.credit {
        return credit_footing(world, &account.label, credit);
    }
    match state(world, &account.label, family) {
        State::Open { five_hour, week, five_hour_resets, week_resets, .. } => {
            let spent = |used: Percent, resets: Option<Unix>| (used >= SUBSCRIPTION_FLOOR).then_some(resets);
            match (spent(five_hour, five_hour_resets), spent(week, week_resets)) {
                (None, None) => {
                    let usd = |used: Percent, points_per_usd: f64| (100.0 - used.0).max(0.0) / points_per_usd.max(f64::MIN_POSITIVE);
                    let worth = usd(five_hour, account.rates.five_hour.points_per_usd()).min(usd(week, account.rates.week.points_per_usd()));
                    Footing::Above { worth: RENEWS * account.weight * worth }
                }
                (Some(five), Some(week)) => Footing::Below { windfall: five.zip(week).map(|(five, week)| five.max(week)) },
                (Some(resets), None) | (None, Some(resets)) => Footing::Below { windfall: resets },
            }
        }
        State::Paused { resumes_at: windfall } | State::WeekOut { resets_at: windfall } | State::ScopeOut { resets_at: windfall, .. } => Footing::Below { windfall },
        State::SignedOut | State::Unknown { .. } => Footing::Unknown,
    }
}

fn credit_footing(world: &World, label: &Label, credit: Credit) -> Footing {
    let refused = world.limits.iter().any(|limit| &limit.account == label && limit.resets_at.is_none_or(|at| at > world.now));
    if refused || credit.left.0 < API_FLOOR.0 { Footing::Below { windfall: credit.next_grant } } else { Footing::Above { worth: credit.left.0 } }
}

pub(super) fn keep(world: &World, session: &Session) -> Option<Action> {
    let own = world.accounts.iter().find(|account| account.label == session.account)?;
    let Footing::Below { .. } = footing(world, own, session.family) else { return None };
    if let Some(to) = roomiest(world, session) {
        return Some(Action::HandOver { to });
    }
    session.parked_since.is_none().then(|| Action::Park { resumes_at: soonest_windfall(world, session) })
}

pub(super) fn strand(world: &World, session: &Session) -> Option<Action> {
    let own = world.accounts.iter().find(|account| account.label == session.account && account.credit.is_some())?;
    let Footing::Below { windfall } = footing(world, own, session.family) else { return None };
    if let Some(to) = roomiest(world, session) {
        return Some(Action::HandOver { to });
    }
    session.parked_since.is_none().then_some(Action::Park { resumes_at: windfall })
}

pub(super) fn roomiest(world: &World, session: &Session) -> Option<Label> {
    let provider = world.accounts.iter().find(|account| account.label == session.account)?.provider;
    world
        .accounts
        .iter()
        .filter(|account| account.provider == provider && account.label != session.account)
        .filter_map(|account| match footing(world, account, session.family) {
            Footing::Above { worth } => Some((worth, &account.label)),
            Footing::Below { .. } | Footing::Unknown => None,
        })
        .max_by(|left, right| left.0.total_cmp(&right.0).then_with(|| right.1.cmp(left.1)))
        .map(|(_, label)| label.clone())
}

fn soonest_windfall(world: &World, session: &Session) -> Option<Unix> {
    world
        .accounts
        .iter()
        .filter_map(|account| match footing(world, account, session.family) {
            Footing::Below { windfall } => windfall.filter(|at| *at > world.now),
            Footing::Above { .. } | Footing::Unknown => None,
        })
        .min()
}
