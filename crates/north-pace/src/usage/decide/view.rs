use std::cmp::Ordering;

use super::world::{Label, Percent, Reading, Seconds, Source, Unix, Window, World};

pub const FRESH_FOR: Seconds = Seconds(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleWhy {
    ResetSinceRead,
    Old,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Limited { resets_at: Option<Unix> },
    NeverRead,
    Stale { read_at: Unix, why: StaleWhy },
    Fresh { used: Percent, resets_at: Option<Unix>, read_at: Unix, source: Source },
}

pub fn view(world: &World, account: &Label, window: Window) -> View {
    let limits = || world.limits.iter().filter(|limit| &limit.account == account && limit.window == window);
    let readings = || world.readings.iter().filter(|reading| &reading.account == account && reading.window == window);
    if let Some(resets_at) = limits().filter_map(|limit| limit.resets_at).filter(|resets_at| *resets_at > world.now).max() {
        return View::Limited { resets_at: Some(resets_at) };
    }
    if limits().any(|limit| limit.resets_at.is_none() && readings().all(|reading| limit.at > reading.read_at)) {
        return View::Limited { resets_at: None };
    }
    let Some(newest) = readings().min_by(|left, right| newest_first(left, right)) else {
        return View::NeverRead;
    };
    if newest.resets_at.is_some_and(|resets_at| resets_at <= world.now) {
        return View::Stale { read_at: newest.read_at, why: StaleWhy::ResetSinceRead };
    }
    if (world.now.0 - newest.read_at.0).abs() > FRESH_FOR.0 {
        return View::Stale { read_at: newest.read_at, why: StaleWhy::Old };
    }
    View::Fresh { used: newest.used, resets_at: newest.resets_at, read_at: newest.read_at, source: newest.source }
}

fn newest_first(left: &Reading, right: &Reading) -> Ordering {
    right
        .read_at
        .cmp(&left.read_at)
        .then_with(|| right.used.0.total_cmp(&left.used.0))
        .then_with(|| later_first(left.resets_at, right.resets_at))
        .then_with(|| left.source.cmp(&right.source))
        .then_with(|| left.row.cmp(&right.row))
}

fn later_first(left: Option<Unix>, right: Option<Unix>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.cmp(&left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
