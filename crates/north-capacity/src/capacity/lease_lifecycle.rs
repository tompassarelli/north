//! The phase of each capacity lease (docs/rationale/state-machines.md, entry 9), kept in `lifecycle.db` beside the lease
//! files in the capacity runtime directory, so a record and its lease file share one lifetime (both vanish with the
//! runtime directory at reboot) and `north health` reads the faults from there.
use std::path::Path;
use std::time::Duration;

use north_base::error::NorthResult;
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, Record, Refusal, State, Store};
use serde_json::Value;

pub const DATABASE: &str = "lifecycle.db";

const RECLAIM_GRACE: Duration = Duration::from_secs(10 * 60);
const FROZEN_DWELL: Duration = Duration::from_secs(6 * 3600);
const OVERSTAY_DWELL: Duration = Duration::from_secs(10 * 60);
const SETTLE_DWELL: Duration = Duration::from_secs(5 * 60);
const ABANDONED_DWELL: Duration = Duration::from_secs(10 * 60);
const KEEP_ENDED: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Reserved,
    Running,
    Frozen,
    Overstaying,
    Settling,
    Abandoned,
    Released,
    Reclaimed,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Reserved, Self::Running, Self::Frozen, Self::Overstaying, Self::Settling, Self::Abandoned, Self::Released, Self::Reclaimed];

    fn name(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Running => "running",
            Self::Frozen => "frozen",
            Self::Overstaying => "overstaying",
            Self::Settling => "settling",
            Self::Abandoned => "abandoned",
            Self::Released => "released",
            Self::Reclaimed => "reclaimed",
        }
    }
}

impl Phase {
    pub fn counts(self) -> bool {
        !matches!(self, Self::Released | Self::Reclaimed)
    }

    pub fn ended(self) -> bool {
        !self.counts()
    }
}

pub struct Leased;

impl Lifecycle for Leased {
    const NAME: &'static str = "capacity-lease";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        use Phase::{Abandoned, Frozen, Overstaying, Reclaimed, Released, Reserved, Running, Settling};
        match from {
            None => matches!(to, Reserved | Running | Frozen),
            Some(Reserved) => matches!(to, Reserved | Frozen | Released | Reclaimed),
            Some(Running) => matches!(to, Frozen | Overstaying | Settling | Reclaimed),
            Some(Frozen) => matches!(to, Reserved | Running | Overstaying | Settling | Released | Reclaimed),
            Some(Overstaying) => matches!(to, Settling | Reclaimed),
            Some(Settling) => matches!(to, Released | Reclaimed),
            Some(Abandoned) => matches!(to, Released | Reclaimed),
            Some(Released | Reclaimed) => false,
        }
    }

    fn dwell(state: Phase) -> Option<Duration> {
        match state {
            Phase::Frozen => Some(FROZEN_DWELL),
            Phase::Overstaying => Some(OVERSTAY_DWELL),
            Phase::Settling => Some(SETTLE_DWELL),
            Phase::Abandoned => Some(ABANDONED_DWELL),
            Phase::Reserved | Phase::Running | Phase::Released | Phase::Reclaimed => None,
        }
    }

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::Settling).then_some(Phase::Abandoned)
    }

    fn keep(state: Phase) -> Option<Duration> {
        state.ended().then_some(KEEP_ENDED)
    }
}

#[derive(Debug, PartialEq)]
pub struct Impossible(pub String);

pub fn classify(lease: &Value) -> Result<Phase, Impossible> {
    let frozen = match lease.get("frozenAt") {
        None | Some(Value::Null) => false,
        Some(at) if at.is_u64() => true,
        Some(other) => return Err(Impossible(format!("frozenAt is {other}, not a time"))),
    };
    let run = match lease.get("kind").and_then(Value::as_str) {
        Some("run") => true,
        Some(_) => false,
        None => return Err(Impossible("the lease names no kind".to_owned())),
    };
    Ok(match (frozen, run) {
        (true, _) => Phase::Frozen,
        (false, true) => Phase::Running,
        (false, false) => Phase::Reserved,
    })
}

fn millis(at_ms: f64) -> Timestamp {
    Timestamp::from_unix_millis(at_ms as i64)
}

fn reclaim_by(expires_at: Option<f64>) -> Option<Timestamp> {
    expires_at.map(|expires| millis(expires + RECLAIM_GRACE.as_millis() as f64))
}

pub struct Ledger {
    store: Store,
}

impl Ledger {
    pub fn open(root: &Path) -> NorthResult<Self> {
        Ok(Self { store: Store::open(&root.join(DATABASE))? })
    }

    pub fn record(&self, id: &str) -> NorthResult<Option<Record<Phase>>> {
        self.store.current::<Leased>(id)
    }

    pub fn phase(&self, id: &str) -> NorthResult<Option<Phase>> {
        Ok(self.record(id)?.map(|record| record.state))
    }

    pub fn counting(&self) -> NorthResult<Vec<Record<Phase>>> {
        Ok(self.store.instances::<Leased>()?.into_iter().filter(|record| record.state.counts()).collect())
    }

    pub fn advance(&mut self, id: &str, to: Phase, cause: &str, expires_at: Option<f64>) -> NorthResult<Option<Record<Phase>>> {
        let deadline = matches!(to, Phase::Reserved | Phase::Running).then(|| reclaim_by(expires_at)).flatten();
        match self.store.transition_by::<Leased>(id, to, cause, Timestamp::now(), deadline)? {
            Ok(record) => Ok(Some(record)),
            Err(Refusal::Held { .. }) => Ok(None),
            Err(Refusal::Illegal { from, to }) => {
                eprintln!("machine-capacity: lease {id} may not move from {} to {}", from.map_or("nothing", Phase::name), to.name());
                Ok(None)
            }
        }
    }

    pub fn adopt(&mut self, lease: &Value) -> NorthResult<Option<Phase>> {
        let id = lease.get("id").and_then(Value::as_str).unwrap_or_default();
        if let Some(phase) = self.phase(id)? {
            return Ok(Some(phase));
        }
        match classify(lease) {
            Ok(phase) => {
                let expires = lease.get("expiresAt").and_then(Value::as_f64);
                let cause = format!("adopted from the lease file, which carried kind {} and {}", lease["kind"], if phase == Phase::Frozen { "frozenAt" } else { "no frozenAt" });
                Ok(self.advance(id, phase, &cause, expires)?.map(|record| record.state))
            }
            Err(Impossible(why)) => {
                eprintln!("machine-capacity: lease {id} matches no phase: {why}");
                Ok(None)
            }
        }
    }
}

pub fn frozen_since(root: &Path, id: &str) -> Option<i64> {
    let record = Ledger::open(root).ok()?.record(id).ok()??;
    (record.state == Phase::Frozen).then(|| record.since.unix_millis())
}
