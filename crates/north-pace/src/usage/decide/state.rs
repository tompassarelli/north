use super::view::{View, view};
use super::world::{Label, ModelFamily, Percent, Unix, Window, World};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    SignedOut,
    WeekOut { resets_at: Option<Unix> },
    ScopeOut { family: ModelFamily, resets_at: Option<Unix> },
    Paused { resumes_at: Option<Unix> },
    Unknown { window: Window, read_at: Option<Unix> },
    Open { five_hour: Percent, week: Percent, five_hour_resets: Option<Unix>, week_resets: Option<Unix>, not_started: bool },
}

pub fn state(world: &World, account: &Label, family: ModelFamily) -> State {
    let signed_in = world.accounts.iter().any(|candidate| &candidate.label == account && !candidate.signed_in.is_empty());
    if !signed_in {
        return State::SignedOut;
    }
    let week = view(world, account, Window::Week);
    let scoped = view(world, account, Window::Scoped(family));
    let five_hour = view(world, account, Window::FiveHour);
    if let Some(resets_at) = out(week) {
        return State::WeekOut { resets_at };
    }
    if let Some(resets_at) = out(scoped) {
        return State::ScopeOut { family, resets_at };
    }
    if let Some(resumes_at) = out(five_hour) {
        return State::Paused { resumes_at };
    }
    let View::Fresh { used: week, resets_at: week_resets, .. } = week else {
        return State::Unknown { window: Window::Week, read_at: read_at(week) };
    };
    match five_hour {
        View::Fresh { used, resets_at, .. } => State::Open { five_hour: used, week, five_hour_resets: resets_at, week_resets, not_started: false },
        _ => State::Open { five_hour: Percent(0.0), week, five_hour_resets: None, week_resets, not_started: true },
    }
}

fn out(view: View) -> Option<Option<Unix>> {
    match view {
        View::Limited { resets_at } => Some(resets_at),
        View::Fresh { used, resets_at, .. } if used.0 >= 100.0 => Some(resets_at),
        _ => None,
    }
}

fn read_at(view: View) -> Option<Unix> {
    match view {
        View::Stale { read_at, .. } | View::Fresh { read_at, .. } => Some(read_at),
        View::Limited { .. } | View::NeverRead => None,
    }
}
