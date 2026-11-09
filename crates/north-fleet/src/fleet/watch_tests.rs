use std::sync::Mutex;

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::gate_lifecycle::GatePhase;
use crate::fleet::gates::{Closure, Gate, GateBook, GateRequest};
use crate::fleet::records::write_record;
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

static CHARGED: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());

impl RunSink for Books0 {
    fn record_run(&self, run: &RunRecord) -> NorthResult<u64> {
        CHARGED.lock().unwrap().push((run.session.clone(), run.account.clone()));
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

    fn last_receipt(&self, _session: &str, _prefix: &str) -> NorthResult<Option<String>> {
        Ok(None)
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

    fn move_gate(&self, _id: i64, _to: GatePhase, _cause: &str, _at_ms: u64) -> NorthResult<bool> {
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
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let held = Arc::new(crate::fleet::claims::Held::default());
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), Arc::new(Told::default()), held, root.path().to_path_buf(), host.clone()).reading_tickets(Arc::new(Unlanded(unlanded)));
    watch.views.insert("s".into(), View { reply: Some(reply.to_owned()), ..View::of("s") });
    watch.follow(&TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals: Default::default() });
    watch.close();
    let record = crate::fleet::records::read_record(&host).unwrap();
    record.ended()
}

fn watched(reply: &str) -> (tempfile::TempDir, PathBuf, TurnWatch) {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let held = Arc::new(crate::fleet::claims::Held::default());
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), Arc::new(Told::default()), held, root.path().to_path_buf(), host.clone()).reading_tickets(Arc::new(Unlanded(Vec::new())));
    watch.views.insert("s".into(), View { reply: Some(reply.to_owned()), ..View::of("s") });
    watch.follow(&TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals: Default::default() });
    watch.turns.observe(TurnSignal::TaskStarted { conversation: "s".into(), task: "landing-loop".into() }, now_ms());
    (root, host, watch)
}

#[test]
fn a_finished_session_retires_at_close_or_at_its_idle_limit_though_its_cli_still_runs_a_background_task_but_one_with_an_unowned_next_or_unlanded_ticket_or_still_unfinished_stays() {
    for reply in ["Done: landed abc123.\nNext: run the bench on gex45", "Split: landed the parser\nNext: none", "Not done: red\nStop: landing\nNext: none", "Done: landed abc123.", "Still waiting on the build.\nNext: none"] {
        assert!(!closed_after(reply, Vec::new()), "{reply}");
    }
    assert!(!closed_after("Done: queued the second lane\nNext: none", vec!["claude/land-second"]));
    let limit = Some(Duration::from_secs(600));
    let (_root, host, mut finished) = watched("Done: landed abc123.\nNext: none");
    assert!(matches!(finished.rest(Duration::from_secs(599), limit), Rest::Wait));
    assert!(matches!(finished.rest(Duration::from_secs(601), limit), Rest::End), "the landing loop a finished worker left running does not hold its host past --idle-exit");
    let record = crate::fleet::records::read_record(&host).unwrap();
    assert_eq!(record.phase(), Some(Phase::IdleExited), "{record:?}");

    let (_root, _host, mut answering) = watched("Done: landed abc123.\nNext: none");
    answering.turns.observe(TurnSignal::Started { conversation: "s".into() }, now_ms());
    assert!(matches!(answering.rest(Duration::from_secs(601), limit), Rest::Wait), "a new turn on a finished session is never cut off");

    let (_root, _host, mut monitoring) = watched("Not done: CI is still running; its monitor wakes me.");
    assert!(matches!(monitoring.rest(Duration::from_secs(3 * 600), limit), Rest::Wait), "an unfinished session's own monitor still wakes it");
}

struct Working(usize);

impl Scope for Working {
    fn detached(&self) -> Vec<Detached> {
        Vec::new()
    }

    fn cli_work(&self) -> usize {
        self.0
    }
}

fn told_after(reply: &str, task: bool, cli_work: usize, close: bool) -> usize {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let held = Arc::new(crate::fleet::claims::Held::default());
    let courier = Arc::new(Told::default());
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), courier.clone(), held, root.path().to_path_buf(), host).reading_scope(Arc::new(Working(cli_work)));
    watch.quiet.insert("s".into(), Quiet::after(1, reply.to_owned(), true, None, Duration::ZERO, Vec::new()));
    if task {
        watch.turns.observe(TurnSignal::TaskStarted { conversation: "s".into(), task: "landing-watch".into() }, now_ms());
    }
    if close {
        watch.close();
    } else {
        watch.speak_up(State::Idle, Duration::from_secs(1));
        watch.finish_probes();
    }
    courier.0.lock().unwrap().len()
}

#[test]
fn a_session_that_ended_its_turn_on_its_own_background_work_is_not_reported_stalled_but_one_without_it_still_is() {
    let waiting = "Started the landing watch; it wakes me when the lane lands.";
    assert_eq!(told_after(waiting, false, 0, false), 1, "no background work and no report word");
    assert_eq!(told_after(waiting, false, 2, false), 0, "its CLI still runs the loop");
    assert_eq!(told_after(waiting, true, 0, true), 0, "its stream still holds the task when the host ends");
    assert_eq!(told_after(waiting, false, 0, true), 1, "the host ends with nothing running");
    assert_eq!(told_after("Done: landed abc123.", false, 2, false), 0, "a final report needs no notice");
}

// KEEP(bug): on 2026-11-07 a turn was charged to the account its session record held when the turn ended, so a session moved between orgs billed the org it moved onto for tokens the org it left served (north#792)
#[test]
fn a_turn_is_charged_to_the_account_its_host_started_with_not_the_one_the_record_holds_when_it_ends() {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("moved-worker");
    std::fs::create_dir_all(&host).unwrap();
    std::fs::write(host.join("host.account"), "api:proton").unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "moved-worker", "provider": "claude", "account": "api:gmail", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), Arc::new(Told::default()), Arc::new(crate::fleet::claims::Held::default()), root.path().to_path_buf(), host);
    for signal in [TurnSignal::Started { conversation: "s".into() }, TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals: Default::default() }] {
        if let Some(turn) = watch.turns.observe(signal, now_ms()) {
            watch.record(turn);
        }
    }
    let charged: Vec<_> = CHARGED.lock().unwrap().iter().filter(|(session, _)| session == "moved-worker").map(|(_, account)| account.clone()).collect();
    assert_eq!(charged, [Some("api:proton".to_owned())]);
}

struct Transport(Vec<Detached>);

impl Scope for Transport {
    fn detached(&self) -> Vec<Detached> {
        self.0.clone()
    }

    fn cli_work(&self) -> usize {
        self.0.len()
    }
}

struct Closed;

impl crate::fleet::waits::Probe for Closed {
    fn result(&self, _wait: &crate::fleet::waits::Registered) -> Option<String> {
        Some("ended".into())
    }
}

#[test]
fn a_closed_transport_task_wakes_no_session() {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let transport = Detached { pid: 41, started: 1, command: "ssh: /run/user/1000/north-ssh-demo [mux]".into() };
    let build = Detached { pid: 42, started: 1, command: "cargo test -p north-fleet".into() };
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), Arc::new(Told::default()), Arc::new(crate::fleet::claims::Held::default()), root.path().to_path_buf(), host).reading_scope(Arc::new(Transport(vec![transport, build.clone()])));
    let _ = watch.rest(Duration::from_secs(1), None);
    let told = Mutex::new(Vec::new());
    let settled = Waits::in_root(root.path()).settle(&Closed, &|name| name == "worker", &|name, text| {
        told.lock().unwrap().push((name.to_owned(), text.to_owned()));
        Ok(())
    });
    assert_eq!(settled.iter().map(|settled| settled.run.clone()).collect::<Vec<_>>(), [build.run()]);
    let told = told.into_inner().unwrap();
    assert_eq!(told.len(), 1);
    assert!(told[0].1.contains("cargo test") && !told[0].1.contains("ssh"), "{told:?}");
}
