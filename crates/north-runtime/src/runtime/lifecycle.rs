//! The runtime promotion machines on this host (docs/rationale/state-machines.md, entry 5), each kept in `ledger.db`
//! through `north_base::lifecycle`: a runtime's journey to `current` and back, the core-loop probes that judge it on a
//! host, and the one judgement settle makes of each landing.
use super::failure::{Failure, failed};
use super::layout::{self, Layout};
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, Record, Refusal, State, Store};
use std::time::Duration;

mod adopt;
#[cfg(test)]
mod promotion_lifecycle;

const BUILD_DWELL: Duration = Duration::from_secs(35 * 60);
const INSTALL_DWELL: Duration = Duration::from_secs(30 * 60);
const CANARY_DWELL: Duration = Duration::from_secs(8 * 60);
const JUDGEMENT_DWELL: Duration = Duration::from_secs(15 * 60);

/// One runtime, by its commit, on this host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Promotion {
    Building,
    Installing,
    Live,
    Superseded,
    BuildFailed,
    InstallFailed,
    RolledBack,
    Unfinished,
}

impl State for Promotion {
    const ALL: &'static [Self] = &[Self::Building, Self::Installing, Self::Live, Self::Superseded, Self::BuildFailed, Self::InstallFailed, Self::RolledBack, Self::Unfinished];

    fn name(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Installing => "installing",
            Self::Live => "live",
            Self::Superseded => "superseded",
            Self::BuildFailed => "build-failed",
            Self::InstallFailed => "install-failed",
            Self::RolledBack => "rolled-back",
            Self::Unfinished => "unfinished",
        }
    }
}

impl Promotion {
    /// A state that keeps its runtime from promotion again until its hold runs out.
    pub(super) fn holds_back(self) -> bool {
        matches!(self, Self::BuildFailed | Self::InstallFailed | Self::RolledBack)
    }
}

pub(super) struct Promoting;

impl Lifecycle for Promoting {
    const NAME: &'static str = "runtime-promotion";
    type State = Promotion;

    fn legal(from: Option<Promotion>, to: Promotion) -> bool {
        use Promotion::*;
        matches!((from, to), (None, _) | (Some(Building), Installing | BuildFailed | Unfinished) | (Some(Installing), Live | InstallFailed | Unfinished) | (Some(Live), Superseded | RolledBack | Installing) | (Some(Superseded), Live | Building | Installing) | (Some(BuildFailed | InstallFailed | RolledBack | Unfinished), Building | Installing))
    }

    fn dwell(state: Promotion) -> Option<Duration> {
        match state {
            Promotion::Building => Some(BUILD_DWELL),
            Promotion::Installing => Some(INSTALL_DWELL),
            _ => None,
        }
    }

    fn orphan(state: Promotion) -> Option<Promotion> {
        matches!(state, Promotion::Building | Promotion::Installing).then_some(Promotion::Unfinished)
    }
}

/// The core-loop probes run for a runtime on a host, by `HOST/SHA`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Canary {
    Judging,
    Passed,
    RolledBack,
    Stuck,
    Unjudged,
    Trusted,
}

impl State for Canary {
    const ALL: &'static [Self] = &[Self::Judging, Self::Passed, Self::RolledBack, Self::Stuck, Self::Unjudged, Self::Trusted];

    fn name(self) -> &'static str {
        match self {
            Self::Judging => "judging",
            Self::Passed => "passed",
            Self::RolledBack => "rolled-back",
            Self::Stuck => "stuck",
            Self::Unjudged => "unjudged",
            Self::Trusted => "trusted",
        }
    }
}

impl Canary {
    pub(super) fn judged(self) -> bool {
        matches!(self, Self::Passed | Self::Stuck | Self::Trusted)
    }
}

pub(super) struct Canarying;

impl Lifecycle for Canarying {
    const NAME: &'static str = "runtime-canary";
    type State = Canary;

    fn legal(from: Option<Canary>, to: Canary) -> bool {
        use Canary::*;
        matches!((from, to), (None, _) | (Some(Judging), Passed | RolledBack | Stuck | Unjudged) | (Some(RolledBack | Unjudged), Judging | Trusted))
    }

    fn dwell(state: Canary) -> Option<Duration> {
        (state == Canary::Judging).then_some(CANARY_DWELL)
    }

    fn orphan(state: Canary) -> Option<Canary> {
        (state == Canary::Judging).then_some(Canary::Unjudged)
    }
}

/// The settle verdict on a landing, by its commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Judgement {
    Judging,
    Judged,
    Unjudged,
}

impl State for Judgement {
    const ALL: &'static [Self] = &[Self::Judging, Self::Judged, Self::Unjudged];

    fn name(self) -> &'static str {
        match self {
            Self::Judging => "judging",
            Self::Judged => "judged",
            Self::Unjudged => "unjudged",
        }
    }
}

pub(super) struct Judge;

impl Lifecycle for Judge {
    const NAME: &'static str = "runtime-landing-judgement";
    type State = Judgement;

    fn legal(from: Option<Judgement>, to: Judgement) -> bool {
        use Judgement::*;
        matches!((from, to), (None, _) | (Some(Judging), Judged | Unjudged) | (Some(Unjudged), Judging))
    }

    fn dwell(state: Judgement) -> Option<Duration> {
        (state == Judgement::Judging).then_some(JUDGEMENT_DWELL)
    }

    fn orphan(state: Judgement) -> Option<Judgement> {
        (state == Judgement::Judging).then_some(Judgement::Unjudged)
    }
}

/// This host's store, after adopting once whatever the marker files said before the machines existed.
pub(super) fn store(layout: &Layout) -> Result<Store, Failure> {
    let mut store = Store::open(&layout.ledger()).map_err(unreadable)?;
    adopt::adopt(layout, &mut store)?;
    Ok(store)
}

fn unreadable(error: north_base::error::NorthError) -> Failure {
    failed(1, error.to_string())
}

pub(super) fn move_to<L: Lifecycle>(layout: &Layout, instance: &str, to: L::State, cause: &str) -> Result<Record<L::State>, Failure> {
    moved::<L>(store(layout)?.transition::<L>(instance, to, cause, Timestamp::now()).map_err(unreadable)?, instance)
}

pub(super) fn moved<L: Lifecycle>(moved: north_base::lifecycle::Moved<L::State>, instance: &str) -> Result<Record<L::State>, Failure> {
    moved.map_err(|refusal| match refusal {
        Refusal::Illegal { from, to } => failed(1, format!("{} {instance}: no move from {} to {}", L::NAME, from.map_or("nothing", State::name), to.name())),
        Refusal::Held { state, owner } => failed(1, format!("{} {instance} is {} and held by {owner}", L::NAME, state.name())),
    })
}

pub(super) fn record_of<L: Lifecycle>(layout: &Layout, instance: &str) -> Result<Option<Record<L::State>>, Failure> {
    store(layout)?.current::<L>(instance).map_err(unreadable)
}

pub(super) fn state_of<L: Lifecycle>(layout: &Layout, instance: &str) -> Result<Option<L::State>, Failure> {
    Ok(record_of::<L>(layout, instance)?.map(|record| record.state))
}

/// The runtime a rollback of `failed_sha` returns to: the newest one a promotion replaced.
pub(super) fn previous(layout: &Layout, failed_sha: &str, usable: impl Fn(&str) -> bool) -> Result<Option<String>, Failure> {
    let records = store(layout)?.instances::<Promoting>().map_err(unreadable)?;
    Ok(records.into_iter().rev().find(|record| record.state == Promotion::Superseded && record.instance != failed_sha && usable(&record.instance)).map(|record| record.instance))
}

/// Makes the runtime `current` points at a live one, adopting it when it has no record; whether it was already live.
pub(super) fn confirm_current(layout: &Layout, sha: &str) -> Result<bool, Failure> {
    match state_of::<Promoting>(layout, sha)? {
        Some(Promotion::Live) => Ok(true),
        Some(_) => Ok(false),
        None => move_to::<Promoting>(layout, sha, Promotion::Live, "adopted from the current link, which had no record").map(|_| true),
    }
}

/// `target` passed its health check and `agents sync`, so it is live and the runtime it replaced is superseded.
pub(super) fn went_live(layout: &Layout, target: &str, replaced: Option<&str>) -> Result<(), Failure> {
    move_to::<Promoting>(layout, target, Promotion::Live, "passed its health check and agents sync")?;
    if let Some(replaced) = replaced.filter(|replaced| *replaced != target)
        && state_of::<Promoting>(layout, replaced)? == Some(Promotion::Live)
    {
        move_to::<Promoting>(layout, replaced, Promotion::Superseded, &format!("replaced by {}", super::layout::short(target)))?;
    }
    Ok(())
}

/// The runtime a host rolled back to after `failed` failed is trusted there and never judged again.
pub(super) fn trust(layout: &Layout, host: &str, previous: &str, failed: &str) {
    let instance = format!("{host}/{previous}");
    if !layout::is_sha(previous) || state_of::<Canarying>(layout, &instance).ok().flatten().is_some_and(Canary::judged) {
        return;
    }
    if let Err(failure) = move_to::<Canarying>(layout, &instance, Canary::Trusted, &format!("the rollback target after {failed} failed; trusted, not re-judged")) {
        eprintln!("north runtime canary: {host}: {}: {}", layout::short(previous), failure.message);
    }
}
