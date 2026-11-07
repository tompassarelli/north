//! The phase of each issue's claim as a north_base::lifecycle machine, kept in the ledger beside the `claims` table
//! (docs/rationale/state-machines.md): every writer of a claim row records the move it made and why, and a claim
//! that nobody renewed or released stays visible to `north health` once it outlives [`HELD_FOR`].
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use north_base::lifecycle::{Lifecycle, Refusal, State, Store};

use super::{Claim, Standing};
use crate::foundation::clock::Timestamp;

/// The longest a claim stays held with no renewal, handoff or release: twice [`super::STALE_AFTER_S`].
pub const HELD_FOR: Duration = Duration::from_secs(2 * super::STALE_AFTER_S as u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// An owner works the issue; renewing or handing it on keeps it here.
    Held,
    /// The row stays, but its issue is closed or gone or its owner is not a live session, so it holds nothing.
    Void,
    /// The owner gave the issue up and the row is gone.
    Released,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Held, Self::Void, Self::Released];

    fn name(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Void => "void",
            Self::Released => "released",
        }
    }
}

pub struct Claiming;

impl Lifecycle for Claiming {
    const NAME: &'static str = "claim";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        matches!((from, to), (None | Some(Phase::Held), _) | (Some(Phase::Void | Phase::Released), Phase::Held) | (Some(Phase::Void), Phase::Released))
    }

    fn dwell(state: Phase) -> Option<Duration> {
        (state == Phase::Held).then_some(HELD_FOR)
    }

    fn orphan(_: Phase) -> Option<Phase> {
        None
    }
}

/// One move a writer made to a claim row, recorded once its row write has committed.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub item: String,
    pub to: Phase,
    pub cause: String,
}

impl Moved {
    pub fn held(item: &str, cause: String) -> Self {
        Self { item: item.to_owned(), to: Phase::Held, cause }
    }

    pub fn released(item: &str, cause: String) -> Self {
        Self { item: item.to_owned(), to: Phase::Released, cause }
    }
}

fn fail(error: impl std::fmt::Display) -> String {
    format!("claim lifecycle: {error}")
}

/// Records `moves` in the ledger at `database`. A move the machine refuses is already logged and reported by
/// `north health`, so it is not an error here.
pub fn record(database: &Path, moves: &[Moved], now: Timestamp) -> Result<(), String> {
    if moves.is_empty() {
        return Ok(());
    }
    let mut store = Store::open(database).map_err(fail)?;
    for moved in moves {
        match store.transition::<Claiming>(&moved.item, moved.to, &moved.cause, now).map_err(fail)? {
            Ok(_) | Err(Refusal::Illegal { .. }) => {}
            Err(Refusal::Held { .. }) => unreachable!("a claim phase has no holder process"),
        }
    }
    Ok(())
}

/// Brings every claim's record in line with its row and standing: a row with no record is adopted at the phase its
/// standing gives it, a held row whose issue or owner is gone moves to void, and a record whose row is gone is
/// released. `rows` are the claims to settle with their standing; `present` is every item that has a row.
pub fn settle(database: &Path, rows: &[(Claim, Standing)], present: &BTreeSet<String>, now: Timestamp) -> Result<(), String> {
    let store = Store::open(database).map_err(fail)?;
    let mut moves = Vec::new();
    for (claim, standing) in rows {
        let current = store.current::<Claiming>(&claim.key()).map_err(fail)?.map(|record| record.state);
        let since = Timestamp::from_unix_seconds(claim.since).iso();
        let (wanted, cause) = match standing {
            Standing::Void(why) => (Phase::Void, format!("{why}; the claim by {} since {since}", claim.owner)),
            Standing::Live | Standing::Stale { .. } => (Phase::Held, format!("the claims row owned by {} since {since} is held again", claim.owner)),
        };
        match current {
            Some(phase) if phase == wanted => {}
            None => moves.push(Moved { item: claim.key(), to: wanted, cause: format!("adopted from the claims row owned by {} since {since}: {cause}", claim.owner) }),
            Some(_) => moves.push(Moved { item: claim.key(), to: wanted, cause }),
        }
    }
    for record in store.instances::<Claiming>().map_err(fail)? {
        if record.state != Phase::Released && !present.contains(&record.instance) {
            moves.push(Moved::released(&record.instance, "its claims row is gone".to_owned()));
        }
    }
    drop(store);
    record(database, &moves, now)
}

#[cfg(test)]
mod claim_lifecycle {
    use north_base::lifecycle::FaultKind;

    use super::*;

    fn at(hours: i64) -> Timestamp {
        Timestamp::from_unix_seconds(1_800_000_000 + hours * 3600)
    }

    #[test]
    fn a_claim_released_twice_refuses_the_second_release_and_health_reports_it_for_an_hour() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("threads.db");
        record(&path, &[Moved::held("north#9", "claimed by worker".into())], at(0)).unwrap();
        record(&path, &[Moved::released("north#9", "released by worker".into())], at(1)).unwrap();

        record(&path, &[Moved::released("north#9", "released again by a sweep".into())], at(2)).unwrap();

        let store = Store::open(&path).unwrap();
        let current = store.current::<Claiming>("north#9").unwrap().unwrap();
        assert_eq!((current.state, current.cause.as_str(), current.seq), (Phase::Released, "released by worker", 2));
        let faults = store.faults(at(2)).unwrap();
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].kind, FaultKind::Illegal { to: "released".into(), at_ms: at(2).unix_millis() });
        assert!(store.faults(Timestamp::from_unix_seconds(at(2).unix_seconds() + 3601)).unwrap().is_empty());
    }

    #[test]
    fn a_claim_nobody_renewed_is_stuck_past_its_hold_and_a_renewal_clears_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("threads.db");
        record(&path, &[Moved::held("north#7", "claimed by worker".into()), Moved::held("north#8", "claimed by worker".into())], at(0)).unwrap();
        record(&path, &[Moved::held("north#8", "renewed by worker".into())], at(40)).unwrap();

        let store = Store::open(&path).unwrap();
        let stuck = store.faults(at(49)).unwrap();

        assert_eq!(stuck.iter().map(|fault| (fault.instance.as_str(), matches!(fault.kind, FaultKind::Overdue { .. }))).collect::<Vec<_>>(), [("north#7", true)]);
        assert_eq!(store.history("claim", "north#8").unwrap().iter().map(|moved| moved.cause.as_str()).collect::<Vec<_>>(), ["claimed by worker", "renewed by worker"]);
    }
}
