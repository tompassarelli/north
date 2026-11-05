use std::sync::Mutex;

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::gates::{Closure, Gate, GateBook, GateRequest};
use crate::fleet::receipts::ReceiptBook;
use crate::fleet::turns::RunSink;

use super::*;

struct Leases(Result<bool, &'static str>);

impl JobLeases for Leases {
    fn leased(&self, _session: &str, _machine: Option<&Machine>) -> NorthResult<bool> {
        self.0.map_err(|why| NorthError::Refused(why.into()))
    }
}

#[derive(Default)]
struct Told(Mutex<Vec<String>>);

impl Courier for Told {
    fn notify(&self, parent: &str, _text: String, _delivery: Delivery) {
        self.0.lock().unwrap().push(parent.to_owned());
    }

    fn ask(&self, _request: GateRequest) {}

    fn answer(&self, _host_dir: &Path, _frame: String) {}
}

struct Books0;

impl RunSink for Books0 {
    fn record_run(&self, _run: &RunRecord) -> NorthResult<u64> {
        Ok(1)
    }

    fn join_task(&self, _task_ref: &str, _session: &str) -> NorthResult<()> {
        Ok(())
    }
}

impl ReceiptBook for Books0 {
    fn claim_receipt(&self, _session: &str, _key: &str, _claimed_ms: u64) -> NorthResult<bool> {
        Ok(true)
    }
}

impl GateBook for Books0 {
    fn open_gate(&self, _request: &GateRequest, _opened_ms: u64) -> NorthResult<i64> {
        Ok(0)
    }

    fn gate(&self, _id: i64) -> NorthResult<Option<Gate>> {
        Ok(None)
    }

    fn gates(&self, _closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        Ok(Vec::new())
    }

    fn close_gate(&self, _id: i64, _closure: &Closure) -> NorthResult<bool> {
        Ok(false)
    }
}

impl Books for Books0 {
    fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
        Ok(Vec::new())
    }

    fn landed_since(&self, _since_ms: u64) -> NorthResult<Vec<Landing>> {
        Ok(Vec::new())
    }

    fn record_landing(&self, _task_ref: &str, _sha: &str, _landed_ms: u64, _wall_ms: Option<u64>) -> NorthResult<bool> {
        Ok(false)
    }
}

fn turn_of(model: Option<&str>, effort: Option<&str>) -> Turn {
    Turn { conversation: "c".into(), model: model.map(str::to_owned), effort: effort.map(str::to_owned), prompt: None, started_ms: 0, ended_ms: 1, wall_ms: 1, turns: 1, tokens: Default::default(), exit: TurnExit::Completed }
}

#[test]
fn a_turn_without_an_observed_model_or_effort_takes_its_sessions_launch_identity() {
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "w", "provider": "claude", "account": "a", "cwd": "/tmp", "model": "claude-opus-5-5", "effort": "medium", "created_ms": 1, "host_pid": null})).unwrap();
    let mut bare = turn_of(None, None);
    identify(&mut bare, Some(&record));
    assert_eq!((bare.model.as_deref(), bare.effort.as_deref()), (Some("claude-opus-5-5"), Some("medium")));
    let mut seen = turn_of(Some("claude-sonnet-5-5"), Some("high"));
    identify(&mut seen, Some(&record));
    assert_eq!((seen.model.as_deref(), seen.effort.as_deref()), (Some("claude-sonnet-5-5"), Some("high")));
    let mut orphan = turn_of(None, None);
    identify(&mut orphan, None);
    assert_eq!((orphan.model, orphan.effort), (None, None));
}

fn tell(leases: Result<bool, &'static str>, courier: Arc<Told>) -> Tell {
    let record = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null})).unwrap();
    Tell {
        lanes: Arc::new(NoLanes),
        courier,
        ledger: Arc::new(Books0),
        jobs: Arc::new(Leases(leases)),
        record,
        parent: "lead".into(),
        state: State::Idle,
        turn: 1,
        reply: "Started the push; waiting on its exit.".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
        quiet: Duration::from_secs(60),
        eta_min: None,
        lines: Vec::new(),
    }
}

#[test]
fn a_lease_that_could_not_be_read_counts_as_leased_so_the_check_stays_unsettled_and_nothing_is_told() {
    let courier = Arc::new(Told::default());
    assert!(!tell(Err("ssh answered exit 255"), courier.clone()).run());
    assert!(courier.0.lock().unwrap().is_empty());
}

#[test]
fn a_held_lease_leaves_the_check_unsettled_and_a_free_one_tells_the_parent_once() {
    let courier = Arc::new(Told::default());
    assert!(!tell(Ok(true), courier.clone()).run());
    assert!(courier.0.lock().unwrap().is_empty());
    assert!(tell(Ok(false), courier.clone()).run());
    assert_eq!(*courier.0.lock().unwrap(), ["lead"]);
}
