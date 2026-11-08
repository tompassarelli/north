//! Each offload offer's lifecycle, kept in the capacity runtime directory's `lifecycle.db` beside the leases: the
//! offer is held by the gate that made it, the gate's timeout or the build host's announce settles it once, and the
//! announce's start of the job here (taking its ticket off this host's queue) commits with the move to Started as a
//! process-bound effect, so a gate killed between the two leaves the offer Abandoned and its start retired, never
//! half done (north#862).
use std::path::Path;
use std::time::Duration;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Delivery, Effect, Leased, Lifecycle, State, Store};
use serde_json::json;

use crate::capacity::lease_lifecycle::DATABASE;

/// The outbox kind of an offer's start on this host.
const START: &str = "offload-start";
const KEEP_SETTLED: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Offer {
    /// Offered to the build hosts, held by the gate that waits on it.
    Offered,
    /// A build host announced it, so it runs there.
    Started,
    /// The gate's timeout came first, no build host took it, or its gate is gone.
    Abandoned,
}

impl State for Offer {
    const ALL: &'static [Self] = &[Self::Offered, Self::Started, Self::Abandoned];

    fn name(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::Started => "started",
            Self::Abandoned => "abandoned",
        }
    }
}

pub(super) struct OffloadOffer;

impl Lifecycle for OffloadOffer {
    const NAME: &'static str = "offload-offer";
    type State = Offer;

    fn legal(from: Option<Offer>, to: Offer) -> bool {
        matches!((from, to), (None, Offer::Offered) | (Some(Offer::Offered), Offer::Started | Offer::Abandoned))
    }

    fn dwell(_: Offer) -> Option<Duration> {
        None
    }

    fn orphan(state: Offer) -> Option<Offer> {
        (state == Offer::Offered).then_some(Offer::Abandoned)
    }

    fn keep(state: Offer) -> Option<Duration> {
        (state != Offer::Offered).then_some(KEEP_SETTLED)
    }
}

/// What starting an announced offer here does: the ticket it takes off this host's queue, set aside instead when a
/// taker was handed the request.
pub(super) struct Start<'a> {
    pub(super) ticket: Option<&'a str>,
    pub(super) handed: bool,
}

fn open(root: &Path) -> Option<Store> {
    Store::open(&root.join(DATABASE)).map_err(|error| eprintln!("machine-capacity: the offload offer lifecycle store is unreadable: {}", error.user_message())).ok()
}

fn moved(outcome: north_base::error::NorthResult<north_base::lifecycle::Moved<Offer>>, id: &str) -> bool {
    match outcome {
        Ok(Ok(_)) => true,
        Ok(Err(refusal)) => {
            eprintln!("machine-capacity: offload offer {id} was not moved: {refusal:?}");
            false
        }
        Err(error) => {
            eprintln!("machine-capacity: offload offer {id} was not moved: {}", error.user_message());
            false
        }
    }
}

/// Records offer `id`, held by this gate until `patience` runs out, first settling each offer whose gate is gone.
pub(super) fn offered(root: &Path, id: &str, patience: Duration) {
    let Some(mut store) = open(root) else { return };
    let now = Timestamp::now();
    if let Ok(offers) = store.instances::<OffloadOffer>() {
        offers.iter().filter(|offer| offer.state == Offer::Offered).for_each(|offer| drop(store.settle::<OffloadOffer>(&offer.instance, now)));
    }
    let deadline = (patience != Duration::MAX).then(|| Timestamp::from_unix_millis(now.unix_millis().saturating_add(i64::try_from(patience.as_millis()).unwrap_or(i64::MAX))));
    moved(store.transition_by::<OffloadOffer>(id, Offer::Offered, "offered to the build hosts", now, deadline), id);
}

/// Moves offer `id` to Started with its start here, then delivers the start through `run`; without a readable store
/// the start runs directly.
pub(super) fn started(root: &Path, id: &str, host: &str, start: &Start<'_>, mut run: impl FnMut(&Start<'_>)) {
    let effect = Effect { kind: START.to_owned(), key: format!("{START}/{id}"), payload: json!({ "ticket": start.ticket, "handed": start.handed }), delivery: Delivery::ProcessBound };
    let committed = open(root).and_then(|mut store| moved(store.transition_with_effects::<OffloadOffer>(id, Offer::Started, &format!("{host} announced it"), Timestamp::now(), None, &[effect]), id).then_some(store));
    let Some(mut store) = committed else {
        run(start);
        return;
    };
    let delivered = store.deliver(START, Timestamp::now(), Duration::from_secs(60), |leased: &Leased| {
        let ticket = leased.effect.payload["ticket"].as_str();
        run(&Start { ticket, handed: leased.effect.payload["handed"].as_bool().unwrap_or(false) });
        Ok(())
    });
    if let Err(error) = delivered {
        eprintln!("machine-capacity: the start of offload offer {id} was not delivered: {}", error.user_message());
    }
}

/// Moves offer `id` to Abandoned for `cause`.
pub(super) fn abandoned(root: &Path, id: &str, cause: &str) {
    if let Some(mut store) = open(root) {
        moved(store.transition::<OffloadOffer>(id, Offer::Abandoned, cause, Timestamp::now()), id);
    }
}
