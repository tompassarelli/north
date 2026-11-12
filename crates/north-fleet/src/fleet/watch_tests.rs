use std::path::Path;
use std::sync::Mutex;

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::detached::Detached;
use crate::fleet::gate_lifecycle::GatePhase;
use crate::fleet::gates::{Closure, Gate, GateBook, GateRequest};
use crate::fleet::records::write_record;
use crate::fleet::receipts::ReceiptBook;
use crate::fleet::turns::RunSink;
use crate::fleet::waits::Run;
use crate::fleet::model::LaunchRequest;

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
    fn notify(&self, parent: &str, text: String, _delivery: Delivery) {
        self.0.lock().unwrap().push(format!("{parent} {text}"));
    }

    fn ask(&self, _request: GateRequest) {}

    fn answer(&self, _host_dir: &Path, _frame: String) {}

    fn launch(&self, words: Vec<String>) {
        self.0.lock().unwrap().push(format!("launch {}", words.join("\n")));
    }

    fn resume(&self, session: &str, text: String) {
        self.0.lock().unwrap().push(format!("resume {session} {text}"));
    }
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

    let (launched, _) = escalations(Some(false), true, Some("small-first"), true);
    let [second] = &launched[..] else { panic!("a small-first attempt whose check failed did not start one second attempt") };
    for part in ["--name\nworker-r2", "--task\nnorth#1", "--route\nafter:worker:opus\n", "Item: north#1 (dates parse)", "Not done: the check failed.", "cargo test -p north-fleet", "test parses_dates ... FAILED", "+fn parse() {}"] {
        assert!(second.contains(part), "the second attempt lacks {part:?}: {second}");
    }
    assert!(second.contains("--cwd\n") && second.contains("/main\n") && !second.contains(crate::fleet::route::NOTE), "the second attempt starts outside the main checkout or routes again: {second}");
    assert!(second.contains("--account\nb\n") && !second.contains("--test"), "the second attempt of a real box lost the first rung's account or became a test launch: {second}");
    let (tested, _) = escalations_of(Some(false), true, Some("haiku:medium,opus:medium"), true, true);
    let [second] = &tested[..] else { panic!("a test launch's failed check did not start one second attempt") };
    assert!(second.contains("--test\n") && second.contains("--account\nb\n") && second.contains("--route\nafter:worker:opus:medium\n"), "the second attempt of a test launch lost --test or the first rung's account: {second}");
    let (cleared, _) = escalations(Some(false), false, Some("small-first"), true);
    assert!(cleared.len() == 1 && cleared[0].contains("kept no output or diff"), "a lane that cleared its own record stopped the escalation: {cleared:?}");
    let (same_model, _) = escalations(Some(false), true, Some("opus:low,opus:medium"), true);
    assert!(same_model.len() == 1 && same_model[0].contains("--route\nafter:worker:opus:medium\n"), "the second rung lost its effort: {same_model:?}");
    for (finished, route) in [(None, Some("small-first")), (Some(true), Some("small-first")), (Some(false), None), (Some(false), Some("after:worker:opus"))] {
        assert_eq!(escalations(finished, true, route, true).0, Vec::<String>::new(), "a lane record forged beside finish record {finished:?} (route {route:?}) started an escalation");
    }
    let (launched, told) = escalations(Some(false), true, Some("small-first"), false);
    assert!(launched.is_empty() && told.iter().any(|text| text.contains("the second rung was not started")), "a first rung outside any checkout started its second rung anyway: {launched:?} {told:?}");
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "task_ref": "north#1"})).unwrap();
    let written = LaunchRequest { prompt: "Route: small-first\nItem: north#1".into(), ..record.relaunch_request() };
    assert_eq!(SessionRecord::launched(&written, None, None, false, 1).first_attempt, None, "a Route: line in the brief's text routed the launch without the flag");
    assert!(SessionRecord::launched(&LaunchRequest { route: Some("small-first".into()), ..written }, None, None, false, 1).first_attempt.is_some(), "the --route flag did not route the launch");
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
fn a_hook_spawned_run_under_the_session_scope_wakes_no_session() {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("proxy");
    std::fs::create_dir_all(&host).unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "proxy", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": null})).unwrap();
    write_record(&host, &record).unwrap();
    let distill = Detached { pid: 41, started: 1 };
    let judge = Detached { pid: 42, started: 1 };
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), Arc::new(Told::default()), Arc::new(crate::fleet::claims::Held::default()), root.path().to_path_buf(), host).reading_scope(Arc::new(Transport(vec![distill, judge])));
    let _ = watch.rest(Duration::from_secs(1), None);
    let waits = Waits::in_root(root.path());
    assert!(waits.of("proxy").is_empty(), "a hook child of the session registers no wait");
    waits.register("proxy", Run::Process { pid: 41, started: 1 }, Path::new("/tmp"), Some("north graph ground distill capture")).unwrap();
    let told = Mutex::new(Vec::new());
    let settled = waits.settle(&Closed, &|name| name == "proxy", &|name, text| {
        told.lock().unwrap().push((name.to_owned(), text.to_owned()));
        Ok(())
    });
    assert!(settled.is_empty(), "a hook child's end settles no wait: {settled:?}");
    assert!(told.into_inner().unwrap().is_empty(), "a hook child's end wakes no session");
    assert!(waits.of("proxy").is_empty(), "the unregistered pid wait is dropped without a message");
}

fn git(dir: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(dir).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn escalations(finished: Option<bool>, lane_record: bool, route: Option<&str>, checkout: bool) -> (Vec<String>, Vec<String>) {
    escalations_of(finished, lane_record, route, checkout, false)
}

fn escalations_of(finished: Option<bool>, lane_record: bool, route: Option<&str>, checkout: bool, test: bool) -> (Vec<String>, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    let (host, main, plain) = (root.path().join("worker"), root.path().join("main"), root.path().join("plain"));
    for dir in [&host, &main, &plain] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
    git(&main, &["worktree", "add", "-q", "-b", "lane", root.path().join("lane").to_str().unwrap()]);
    git(&main, &["config", "branch.lane.north-issue", "north#1"]);
    let attempt = serde_json::json!({"check": "cargo test -p north-fleet", "output": "test parses_dates ... FAILED", "diff": "+fn parse() {}"}).to_string();
    let forged = root.path().join("forged.json");
    std::fs::write(&forged, &attempt).unwrap();
    if lane_record {
        std::fs::write(git(&root.path().join("lane"), &["rev-parse", "--path-format=absolute", "--git-path", "north-attempt.json"]), &attempt).unwrap();
    }
    let verdicts = root.path().join("verdicts");
    if let Some(passed) = finished {
        let record = north_base::platform::finish_record::Finished { session: "worker".into(), command: "cargo test -p north-fleet".into(), head: "abc".into(), passed, at: now_ms() / 1000 };
        north_base::platform::finish_record::FinishRecords(verdicts.clone()).record(&record).unwrap();
    }
    let brief = format!("Item: north#1 (dates parse)\nGoal: parse dates{}", crate::fleet::route::NOTE);
    let cwd = if checkout { &main } else { &plain };
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "b", "verification": test, "cwd": cwd, "model": "claude-haiku-5-5", "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead", "task_ref": "north#1", "route": route, "first_attempt": route.map(|_| brief)})).unwrap();
    write_record(&host, &record).unwrap();
    let courier = Arc::new(Told::default());
    let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), courier.clone(), Arc::new(crate::fleet::claims::Held::default()), root.path().to_path_buf(), host).reading_verdicts(verdicts);
    let mut view = View { reply: Some("Not done: the check failed.\nStop: reasoning".to_owned()), ..View::of("s") };
    let text = format!("finish: ✗ the check `cargo test -p north-fleet` exited 101; NOT committed, NOT queued; the attempt is kept in {}", forged.display());
    view.transcript.entries.push(Entry { kind: EntryKind::ToolResult, text });
    watch.views.insert("s".into(), view);
    for signal in [TurnSignal::Started { conversation: "s".into() }, TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals: Default::default() }] {
        if let Some(turn) = watch.turns.observe(signal, now_ms()) {
            watch.record(turn);
        }
    }
    let told = courier.0.lock().unwrap().clone();
    (told.iter().filter_map(|told| told.strip_prefix("launch ")).map(str::to_owned).collect(), told)
}

fn told_over(prompt: &str, replies: &[&str]) -> Vec<String> {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let seed: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "parent": "lead", "role": "worker"})).unwrap();
    let launched = SessionRecord::launched(&LaunchRequest { prompt: prompt.into(), ..seed.relaunch_request() }, None, Some("s".into()), false, 1);
    write_record(&host, &SessionRecord { host_pid: Some(std::process::id()), host_dir: host.clone(), ..launched }).unwrap();
    let courier = Arc::new(Told::default());
    for reply in replies {
        let mut watch = TurnWatch::new(Arc::new(crate::fleet::fakes::Stream), Arc::new(Books0), courier.clone(), Arc::new(crate::fleet::claims::Held::default()), root.path().to_path_buf(), host.clone()).reading_tickets(Arc::new(Unlanded(Vec::new())));
        watch.views.insert("s".into(), View { reply: Some((*reply).to_owned()), ..View::of("s") });
        for signal in [TurnSignal::Started { conversation: "s".into() }, TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals: Default::default() }] {
            watch.follow(&signal);
            if let Some(turn) = watch.turns.observe(signal, now_ms()) {
                watch.record(turn);
            }
        }
    }
    courier.0.lock().unwrap().clone()
}

/// Terminal-Bench task sam-cell-seg, 2026-11-12 and 2026-11-13: the task named its four arguments, the session's script took them positionally, the grader's named flags made it exit 2, and its closing Done stood on a call the session chose itself.
#[test]
fn a_worker_given_no_done_when_check_has_its_first_done_held_once_and_its_next_report_stands() {
    let bare = told_over("Write /app/seg.py taking weights_path, output_path, rgb_path and csv_path.", &["Done: saved 48 masks.", "Done: the grader's call with named flags printed 48 masks."]);
    let [held, stood] = &bare[..] else { panic!("the bare task's two Done reports were not one hold and one report: {bare:?}") };
    assert_eq!(held, &format!("resume worker {}", done_hold::MESSAGE), "{bare:?}");
    assert!(stood.starts_with("lead [from worker") && stood.contains("the grader's call"), "the report after the hold did not go up: {bare:?}");
    let briefed = told_over("Item: north#1\nDone when: `cargo test` exits 0", &["Done: landed abc123."]);
    assert!(briefed.len() == 1 && briefed[0].starts_with("lead [from worker") && briefed[0].contains("landed abc123"), "a worker with a Done-when line had its Done held: {briefed:?}");
}
