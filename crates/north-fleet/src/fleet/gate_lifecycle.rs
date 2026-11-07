use std::time::Duration;

use north_base::lifecycle::{Lifecycle, State};

use crate::foundation::clock::Timestamp;

use super::gates::{Answerer, GateRequest};

const DELIVERY_DWELL: Duration = Duration::from_secs(5 * 60);
const REDELIVERY_DWELL: Duration = Duration::from_secs(10 * 60);
const DEFAULT_GRACE: Duration = Duration::from_secs(5 * 60);

/// The phase of one gate on its host: waiting for Tom or for its default, its answer on the way to the asking session, or settled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GatePhase {
    AskingTom,
    CountingDown,
    Delivering,
    Undelivered,
    Answered,
    Defaulted,
    Orphaned,
}

impl State for GatePhase {
    const ALL: &'static [Self] = &[Self::AskingTom, Self::CountingDown, Self::Delivering, Self::Undelivered, Self::Answered, Self::Defaulted, Self::Orphaned];

    fn name(self) -> &'static str {
        match self {
            Self::AskingTom => "asking-tom",
            Self::CountingDown => "counting-down",
            Self::Delivering => "delivering",
            Self::Undelivered => "undelivered",
            Self::Answered => "answered",
            Self::Defaulted => "defaulted",
            Self::Orphaned => "orphaned",
        }
    }
}

impl GatePhase {
    pub fn is_open(self) -> bool {
        matches!(self, Self::AskingTom | Self::CountingDown)
    }

    pub fn is_settled(self) -> bool {
        matches!(self, Self::Answered | Self::Defaulted | Self::Orphaned)
    }

    /// The phase a gate opens in: counting down to its default, or waiting for Tom when it has none.
    pub fn opened(request: &GateRequest) -> Self {
        if request.has_default() { Self::CountingDown } else { Self::AskingTom }
    }

    /// The phase a gate rests in once its answer reached the asking session, by who answered.
    pub fn settled(by: &Answerer) -> Self {
        match by {
            Answerer::Session(_) | Answerer::Tom => Self::Answered,
            Answerer::Default => Self::Defaulted,
            Answerer::Orphaned => Self::Orphaned,
        }
    }

    /// When a gate counting down to its default is overdue: the deadline and a few minutes for the timer or a sweep to apply it.
    pub fn deadline(self, request: &GateRequest, opened_ms: u64) -> Option<Timestamp> {
        let due = opened_ms.saturating_add(request.default_after_ms).saturating_add(u64::try_from(DEFAULT_GRACE.as_millis()).unwrap_or(0));
        (self == Self::CountingDown).then(|| Timestamp::from_unix_millis(i64::try_from(due).unwrap_or(i64::MAX)))
    }
}

/// Gates, one instance per gate id in its host's ledger: `legal` is the whole table, a closer holds `Delivering`, and a gate whose closer died is `Undelivered` until a sweep delivers it again.
pub struct GateLife;

impl Lifecycle for GateLife {
    const NAME: &'static str = "gate";
    type State = GatePhase;

    fn legal(from: Option<GatePhase>, to: GatePhase) -> bool {
        use GatePhase::{Answered, AskingTom, CountingDown, Defaulted, Delivering, Orphaned, Undelivered};
        match from {
            None => true,
            Some(AskingTom | CountingDown) => to == Delivering,
            Some(Delivering) => matches!(to, Answered | Defaulted | Orphaned | Undelivered),
            Some(Undelivered) => to == Delivering,
            Some(Answered | Defaulted | Orphaned) => false,
        }
    }

    fn dwell(state: GatePhase) -> Option<Duration> {
        match state {
            GatePhase::Delivering => Some(DELIVERY_DWELL),
            GatePhase::Undelivered => Some(REDELIVERY_DWELL),
            _ => None,
        }
    }

    fn orphan(state: GatePhase) -> Option<GatePhase> {
        (state == GatePhase::Delivering).then_some(GatePhase::Undelivered)
    }
}

#[cfg(test)]
mod tests {
    use north_base::lifecycle::{FaultKind, Liveness, Owner, Refusal, Store};

    use super::*;

    struct Gone(u32);

    impl Liveness for Gone {
        fn alive(&self, owner: &Owner) -> Option<bool> {
            Some(owner.pid != self.0)
        }
    }

    fn owner(pid: u32) -> Owner {
        Owner { host: "gex45".into(), pid, started: 1, booted_ms: 1 }
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(1_800_000_000 + seconds)
    }

    fn request(default_after_ms: u64) -> GateRequest {
        GateRequest { verification: false, issue: None, box_text: None, session: "worker".into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms }
    }

    #[test]
    fn gate_lifecycle_refuses_to_reopen_an_answered_gate_and_health_sees_the_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open_as(&directory.path().join("ledger.db"), owner(10), Box::new(Gone(0))).unwrap();
        store.transition::<GateLife>("7", GatePhase::AskingTom, "opened for Tom", at(0)).unwrap().unwrap();
        store.transition::<GateLife>("7", GatePhase::Delivering, "tom answered 1", at(5)).unwrap().unwrap();
        store.transition::<GateLife>("7", GatePhase::Answered, "delivered", at(6)).unwrap().unwrap();

        let refused = store.transition::<GateLife>("7", GatePhase::Delivering, "a second answer", at(9)).unwrap();

        assert_eq!(refused, Err(Refusal::Illegal { from: Some(GatePhase::Answered), to: GatePhase::Delivering }));
        let record = store.current::<GateLife>("7").unwrap().unwrap();
        assert_eq!((record.state, record.cause.as_str(), record.seq), (GatePhase::Answered, "delivered", 3));
        let faults = store.faults(at(10)).unwrap();
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].kind, FaultKind::Illegal { to: "delivering".into(), at_ms: at(9).unix_millis() });
    }

    #[test]
    fn gate_lifecycle_reports_a_default_nobody_applied_and_an_answer_whose_closer_died() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.db");
        let overdue = request(30 * 60 * 1000);
        let opened = GatePhase::opened(&overdue);
        let mut timerless = Store::open_as(&path, owner(10), Box::new(Gone(0))).unwrap();
        timerless.transition_by::<GateLife>("1", opened, "opened with a default in 30 min", at(0), opened.deadline(&overdue, at(0).unix_millis() as u64)).unwrap().unwrap();
        timerless.transition::<GateLife>("2", GatePhase::AskingTom, "opened for Tom", at(0)).unwrap().unwrap();
        timerless.transition::<GateLife>("2", GatePhase::Delivering, "tom answered 1", at(60)).unwrap().unwrap();

        let mut sweeper = Store::open_as(&path, owner(11), Box::new(Gone(10))).unwrap();
        let faults = sweeper.faults(at(30 * 60 + 5 * 60 + 1)).unwrap();

        assert_eq!(faults.iter().map(|fault| (fault.instance.as_str(), fault.state.as_str())).collect::<Vec<_>>(), [("1", "counting-down"), ("2", "delivering")]);
        assert!(matches!(faults[0].kind, FaultKind::Overdue { .. }));
        assert_eq!(faults[1].kind, FaultKind::Orphaned { owner: owner(10) });
        assert!(sweeper.faults(at(10 * 60)).unwrap().iter().all(|fault| fault.instance == "2"), "a default inside its deadline is not stuck");
        let retaken = sweeper.transition::<GateLife>("2", GatePhase::Delivering, "delivering the answer again", at(2_300)).unwrap();
        assert!(retaken.is_ok());
        let moves: Vec<_> = sweeper.history("gate", "2").unwrap().into_iter().map(|logged| (logged.from, logged.to)).collect();
        assert_eq!(moves[2], (Some("delivering".into()), "undelivered".into()));
    }
}
