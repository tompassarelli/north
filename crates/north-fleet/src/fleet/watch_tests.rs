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

    fn reported(&self, _session: &str) -> NorthResult<bool> {
        Ok(false)
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

struct Unlanded(Vec<&'static str>);

impl Tickets for Unlanded {
    fn unlanded(&self, _session: &str) -> NorthResult<Vec<String>> {
        Ok(self.0.iter().map(|branch| (*branch).to_owned()).collect())
    }
}

fn closed_after(reply: &str, unlanded: Vec<&'static str>) -> bool {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let held = Arc::new(crate::fleet::claims::Held::default());
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), Arc::new(Told::default()), held, root.path().to_path_buf(), host.clone()).reading_tickets(Arc::new(Unlanded(unlanded)));
    watch.views.insert("s".into(), View { reply: Some(reply.to_owned()), ..View::of("s") });
    watch.follow(&TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals: Default::default() });
    watch.close();
    let record = crate::fleet::records::read_record(&host).unwrap();
    assert_eq!(record.ended.is_some(), record.stopped, "{reply}");
    record.stopped
}

#[test]
fn a_session_stays_listed_for_a_bare_next_a_split_a_missing_next_or_an_unlanded_ticket() {
    for reply in ["Done: landed abc123.\nNext: run the bench on gex45", "Split: landed the parser\nNext: none", "Not done: red\nStop: landing\nNext: none", "Done: landed abc123.", "Still waiting on the build.\nNext: none"] {
        assert!(!closed_after(reply, Vec::new()), "{reply}");
    }
    assert!(!closed_after("Done: queued the second lane\nNext: none", vec!["claude/land-second"]));
}
