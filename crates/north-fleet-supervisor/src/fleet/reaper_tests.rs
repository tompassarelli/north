use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::gates::{GateBook, GateRequest};
use crate::fleet::model::{Delivery, Phase};
use crate::fleet::receipts::ReceiptBook;
use crate::fleet::registry::fixtures::live;
use crate::fleet::registry::Sent;
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::runtimes::Runtimes;
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind};
use crate::fleet::records::ARCHIVE;
use crate::fleet::waits::Run;

const HOUR: Duration = Duration::from_secs(3600);

#[derive(Default)]
struct Sight {
    landed: BTreeSet<String>,
    running: BTreeMap<String, Work>,
    lanes: BTreeMap<String, Vec<north_base::land_ticket::Lane>>,
    lanes_ahead: BTreeMap<String, Unlanded>,
}

impl Evidence for Sight {
    fn landed(&self, _cwd: &Path, sha: &str) -> bool {
        self.landed.contains(sha)
    }

    fn running(&self, records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        records.iter().filter_map(|record| Some((record.name.clone(), *self.running.get(&record.name)?))).collect()
    }

    fn lanes(&self) -> BTreeMap<String, Vec<north_base::land_ticket::Lane>> {
        self.lanes.clone()
    }

    fn lane(&self, record: &SessionRecord) -> Option<Unlanded> {
        self.lanes_ahead.get(&record.name).cloned()
    }
}

fn fleet(home: &Path) -> (Fleet, Arc<Paper>) {
    let paper = Arc::new(Paper::default());
    (Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Stream)]), paper.clone()).unwrap().arming(|_| Ok(())), paper)
}

fn said(name: &str, kind: &str, text: &str) -> Value {
    json!({"type": kind, "session_id": name, "message": {"content": [{"type": "text", "text": text}]}})
}

fn parked(fleet: &Fleet, name: &str, parent: Option<&str>, frames: &[Value]) {
    let dir = fleet.root.join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": name, "parent": parent, "host_dir": dir, "stream_offset": 0, "task_ref": "north#636"});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let lines: String = frames.iter().map(|frame| format!("{frame}\n")).collect();
    std::fs::write(dir.join("g1/stream.jsonl"), lines).unwrap();
}

fn reported(fleet: &Fleet, name: &str, parent: Option<&str>, report: &str) {
    let result = json!({"type": "result", "subtype": "success", "session_id": name});
    parked(fleet, name, parent, &[said(name, "user", "Item: north#636"), said(name, "assistant", report), result]);
}

fn retired(reaped: &Reaped) -> Vec<&str> {
    reaped.retired.iter().map(|retired| retired.name.as_str()).collect()
}

fn archived_dir(fleet: &Fleet, name: &str) -> std::path::PathBuf {
    std::fs::read_dir(fleet.root.join(ARCHIVE)).unwrap().flatten().map(|entry| entry.path()).find(|path| path.file_name().unwrap().to_string_lossy().starts_with(&format!("{name}-"))).unwrap()
}

#[test]
fn a_done_report_retires_its_session_only_once_its_parent_took_it_and_each_sha_it_names_is_on_origin_main() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let _lead = live(&fleet, "lead");
    reported(&fleet, "worker", Some("lead"), "Done: the reaper landed as 1af7168df on origin/main.");
    let mut evidence = Sight::default();

    let untaken = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);
    paper.claim_receipt("worker", "turn:1", now_ms()).unwrap();
    let unlanded = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);
    evidence.landed.insert("1af7168df".into());
    let finished = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);

    assert!(retired(&untaken).is_empty() && retired(&unlanded).is_empty(), "{untaken:?} {unlanded:?}");
    assert!(unlanded.kept.iter().any(|kept| kept.name == "worker" && kept.why.contains("1af7168df")), "{unlanded:?}");
    assert_eq!(retired(&finished), ["worker"]);
    assert!(fleet.session("worker").is_err(), "a retired session leaves the registry");
    let archive = archived_dir(&fleet, "worker");
    let record = crate::fleet::records::read_record(&archive).unwrap();
    assert_eq!(record.phase(), Some(Phase::Retired), "{record:?}");
    assert!(std::fs::read_to_string(archive.join("g1/stream.jsonl")).unwrap().contains("1af7168df"), "the transcript is archived with it");
    let note: Value = serde_json::from_slice(&std::fs::read(archive.join("retired.json")).unwrap()).unwrap();
    assert!(note["last"].as_str().unwrap().starts_with("Done: the reaper landed"), "{note}");
    assert_eq!(retired(&reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true)), Vec::<&str>::new(), "the fresh lead stays");
}

#[test]
fn a_not_done_or_split_retires_only_when_it_names_a_stop_its_parent_took_and_new_work_after_a_done_keeps_it() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let _lead = live(&fleet, "lead");
    reported(&fleet, "stopped", Some("lead"), "Split: boxes 1 and 2 landed; box 3 is its own item.\nStop: budget");
    reported(&fleet, "unstopped", Some("lead"), "Not done: the suite is red on main.");
    let result = json!({"type": "result", "subtype": "success", "session_id": "rehired"});
    parked(&fleet, "rehired", Some("lead"), &[said("rehired", "assistant", "Done: nothing to land."), result.clone(), said("rehired", "user", "[from lead] also port the shell test"), said("rehired", "assistant", "Porting it now."), result]);
    for name in ["stopped", "unstopped", "rehired"] {
        paper.claim_receipt(name, "turn:1", now_ms()).unwrap();
    }

    let reaped = reap_after(&fleet, &Sight::default(), 24 * HOUR, now_ms(), true);

    assert_eq!(retired(&reaped), ["stopped"]);
    assert!(fleet.session("unstopped").is_ok() && fleet.session("rehired").is_ok());
}

/// Eight workers parked under the coordinator's hold were retired on 2026-11-12 after a taken Not done report, lanes ahead of main and their parent untold (north#1235).
#[test]
fn a_worker_parked_on_an_unlanded_lane_after_a_not_done_report_stays_and_stale_retirements_tell_their_parent_every_lane_in_one_digest() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let lead = live(&fleet, "lead");
    reported(&fleet, "held", Some("lead"), "Not done: the commit is on the lane but the coordinator holds hand-ins.\nStop: landing");
    paper.claim_receipt("held", "turn:1", now_ms()).unwrap();
    for name in ["exited", "slow"] {
        reported(&fleet, name, Some("lead"), "Done: nothing to land.");
        let record = fleet.root.join(name).join("session.json");
        let mut stopped: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
        stopped["stopped"] = json!(true);
        std::fs::write(&record, stopped.to_string()).unwrap();
    }
    let lane = |path: &str, tip: &str, commits| Unlanded::Ahead(Ahead { path: path.into(), tip: tip.into(), commits });
    let lanes_ahead = [("held".to_owned(), lane("/home/tom/code/north/worktrees/held-lane", "8ffd6add62", 1)), ("exited".to_owned(), lane("/home/tom/code/north/worktrees/exited-lane", "6d0cecb063", 2)), ("slow".to_owned(), Unlanded::Unread("/home/tom/code/north/worktrees/slow-lane".into()))];
    let evidence = Sight { lanes_ahead: lanes_ahead.into(), ..Sight::default() };

    let waiting = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);
    assert!(retired(&waiting).is_empty() && fleet.root.join("held").exists(), "{waiting:?}");
    let stale = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);

    let mut gone = retired(&stale);
    gone.sort_unstable();
    assert_eq!(gone, ["exited", "held"]);
    assert!(fleet.session("slow").is_ok(), "a lane probe that timed out keeps its session");
    let frames = lead.frames.lock().unwrap();
    let digests: Vec<&String> = frames.iter().filter(|frame| frame.contains("lanes hold commits not on origin/main")).collect();
    assert_eq!(digests.len(), 1, "one digest per parent and tick: {frames:?}");
    assert!(digests[0].contains("3 sessions") && digests[0].contains("held: retired; /home/tom/code/north/worktrees/held-lane at 8ffd6add62, 1 commits") && digests[0].contains("exited: retired; /home/tom/code/north/worktrees/exited-lane at 6d0cecb063, 2 commits") && digests[0].contains("slow: kept; its lane /home/tom/code/north/worktrees/slow-lane did not answer"), "{digests:?}");
}

/// north-669-d1955 stopped on `Stop: landing` with its guards lane queued: the reaper keeps it while the lane is open and
/// after the lander decides it until the session is woken, and retires it once the lane is settled (north#790).
#[test]
fn a_session_stopped_on_its_queued_lane_stays_until_the_lander_decides_it_and_wakes_it() {
    let (home, state) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (fleet, paper) = fleet(home.path());
    let _lead = live(&fleet, "lead");
    reported(&fleet, "north-669-d1955", Some("lead"), "Not done: the guards commit bf636ba82 is in the lander queue but not on origin/main, so no box of north#669 is ticked yet.\nStop: landing");
    paper.claim_receipt("north-669-d1955", "turn:1", now_ms()).unwrap();
    let (stream, ended) = (fleet.root.join("north-669-d1955/g1/stream.jsonl"), std::time::SystemTime::now() - Duration::from_secs(600));
    std::fs::File::options().write(true).open(stream).unwrap().set_modified(ended).unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    std::fs::create_dir_all(&queue).unwrap();
    let ticket = queue.join(format!("{}-guards.json", (now_ms() - 12 * 60_000) * 1000));
    std::fs::write(&ticket, json!({"session": "north-669-d1955", "branch": "guards", "queued_micros": (now_ms() - 12 * 60_000) * 1000}).to_string()).unwrap();
    let mut records = north_base::land_ticket::Records::of(&ticket).unwrap();
    records.moved(&ticket, north_base::land_ticket::Phase::Queued, "queued").unwrap();
    let held = |state: &Path| Sight { lanes: north_base::land_ticket::lanes_by_session(&state.join("north/land-queue")).unwrap(), ..Sight::default() };

    let open = reap_after(&fleet, &held(state.path()), 24 * HOUR, now_ms(), false);
    let indexed = crate::fleet::resume::ended_ms(&fleet.session("north-669-d1955").unwrap());
    records.moved(&ticket, north_base::land_ticket::Phase::Failed, "failed: the candidate's tests are red").unwrap();
    let decided = reap_after(&fleet, &held(state.path()), 24 * HOUR, now_ms(), false);
    let settled = reap_after(&fleet, &Sight::default(), 24 * HOUR, now_ms(), false);

    assert_eq!(indexed.map(u128::from), Some(ended.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()), "the reap that indexed the session still reads its end from its stream (north#821)");
    assert!(retired(&open).is_empty() && retired(&decided).is_empty(), "{open:?} {decided:?}");
    assert!(decided.kept.iter().any(|kept| kept.name == "north-669-d1955" && kept.why.contains("lander")), "{decided:?}");
    assert_eq!(retired(&settled), ["north-669-d1955"], "a landing stop with no lane left to wait on finishes: {settled:?}");
}

#[test]
fn a_retired_note_keeps_the_report_not_a_later_reply_to_a_lander_notice() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let _lead = live(&fleet, "lead");
    let result = json!({"type": "result", "subtype": "success", "session_id": "north-725-d0351"});
    let report = "Not done: two of the three boxes on north#725 are still open.\nStop: landing";
    parked(&fleet, "north-725-d0351", Some("lead"), &[said("north-725-d0351", "assistant", report), result.clone(), said("north-725-d0351", "user", "[from service:north-fleet] [north: landed] 988a0d0cef is on origin/main"), said("north-725-d0351", "assistant", "The lander's message confirms the landing I already reported."), result]);
    paper.claim_receipt("north-725-d0351", "turn:1", now_ms()).unwrap();

    assert_eq!(retired(&reap_after(&fleet, &Sight::default(), 24 * HOUR, now_ms(), true)), ["north-725-d0351"]);
    let note: Value = serde_json::from_slice(&std::fs::read(archived_dir(&fleet, "north-725-d0351").join("retired.json")).unwrap()).unwrap();
    assert_eq!(note["last"], report, "the note keeps the report, not a later reply");
}

#[test]
fn a_message_to_a_retired_session_reaches_its_parent_as_one_turn_and_never_revives_it() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, _paper) = fleet(home.path());
    let lead = live(&fleet, "lead");
    reported(&fleet, "worker", Some("lead"), "Done: nothing to land.");
    fleet.retire("worker", "test", Some("Done: nothing to land.")).unwrap();

    let sent = fleet.send_message(&Sender::Tom, "worker", "are you there", Delivery::Queued).unwrap();
    let row = FleetRow { id: format!("{}-{}", now_ms(), new_uuid().unwrap()), kind: RowKind::Message, to: "worker".into(), from: "far".into(), urgent: false, sent_at: now_ms(), text: "from the far host".into(), sender: Sender::Session("peer".into()), gate: None, answer: None, job: None, origin: None, spooled_by: None };
    fleet.hand_in(&row).unwrap();

    assert_eq!(sent, Sent::Retired { heir: "lead".into() });
    assert!(fleet.session("worker").is_err() && !fleet.root.join("worker").exists(), "the retired session is not revived");
    let frames = lead.frames.lock().unwrap();
    assert_eq!(frames.len(), 2, "{frames:?}");
    assert!(frames[0].contains("worker is retired") && frames[0].contains("are you there"), "{frames:?}");
    assert!(frames[1].contains("worker is retired") && frames[1].contains("from the far host"), "{frames:?}");
}

#[test]
fn a_session_retires_when_finished_though_its_cli_still_runs_a_background_loop_when_its_host_ended_it_at_idle_exit_or_when_quiet_past_the_stale_window_unless_a_wait_gate_lane_job_or_unfinished_task_would_wake_it() {
    {
        let home = tempfile::tempdir().unwrap();
        let (fleet, paper) = fleet(home.path());
        for name in ["quiet", "waiting", "gated", "queued", "running"] {
            reported(&fleet, name, None, &format!("{name}: still on it"));
        }
        fleet.waits().register("waiting", Run::Branch("n636-reaper".into()), Path::new("/tmp"), None).unwrap();
        let gate = GateRequest { verification: false, issue: None, box_text: None, session: "gated".into(), question: "Which?".into(), options: vec!["a".into(), "b".into()], recommend: 1, default: 1, default_after_ms: 0, ask_once: None };
        paper.open_gate(&gate, now_ms()).unwrap();
        let queue = home.path().join(".local/state/north/land-queue/560e3f31dbaa2415");
        std::fs::create_dir_all(&queue).unwrap();
        std::fs::write(queue.join("1-queued.json"), json!({"root": "/elsewhere", "branch": "queued-lane", "session": "queued"}).to_string()).unwrap();
        let evidence = Sight { running: [("running".to_owned(), Work::Detached)].into(), ..Sight::default() };
    
        let spent = reap_within(&fleet, &evidence, Duration::ZERO, now_ms(), true, Duration::ZERO, &|| Duration::ZERO);
        let fresh = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);
        let stale = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);
    
        assert!(retired(&spent).is_empty() && spent.deferred == 5, "a reaper out of budget leaves every session to the next tick: {spent:?}");
        assert!(retired(&fresh).is_empty(), "{fresh:?}");
        assert_eq!(retired(&stale), ["quiet"]);
        let why = |name: &str| stale.kept.iter().find(|kept| kept.name == name).map(|kept| kept.why.clone()).unwrap_or_default();
        assert!(why("waiting").contains("branch:n636-reaper") && why("gated").contains("gate") && why("queued").contains("queued-lane") && why("running").contains("background job"), "{stale:?}");
        let note: Value = serde_json::from_slice(&std::fs::read(archived_dir(&fleet, "quiet").join("retired.json")).unwrap()).unwrap();
        assert_eq!(note["last"], "quiet: still on it");
        assert!(note["why"].as_str().unwrap().contains("quiet over 0h"), "{note}");
    }
    let home = tempfile::tempdir().unwrap();
    let (fleet, paper) = fleet(home.path());
    let _lead = live(&fleet, "lead");
    reported(&fleet, "finished", Some("lead"), "Done: the fix landed as 1af7168df on origin/main.");
    reported(&fleet, "monitoring", Some("lead"), "Not done: CI is still running; its monitor wakes me.");
    reported(&fleet, "queued", Some("lead"), "Not done: the fix is written but clippy is queued in capacity admission, and the run will notify me when it ends.\n\nStop: capacity");
    reported(&fleet, "ended", Some("lead"), "Done: the arm result landed as 2bc8279e0.");
    for name in ["finished", "monitoring", "queued", "ended"] {
        paper.claim_receipt(name, "turn:1", now_ms()).unwrap();
    }
    let dir = fleet.root.join("ended");
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap()).unwrap();
    legacy["stopped"] = json!(true);
    legacy["ended"] = json!("idle-exit");
    std::fs::write(dir.join("session.json"), legacy.to_string()).unwrap();
    fleet.waits().register("ended", Run::Process { pid: 1, started: 1 }, Path::new("/tmp"), Some("sh -c d=$0 u=$1 limit=$2")).unwrap();
    let running = ["finished", "monitoring", "queued"].map(|name| (name.to_owned(), Work::Cli)).into();
    let evidence = Sight { landed: ["1af7168df".to_owned()].into(), running, ..Sight::default() };

    let reaped = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);

    let mut names = retired(&reaped);
    names.sort_unstable();
    assert_eq!(names, ["ended", "finished"], "{reaped:?}");
    assert!(reaped.kept.iter().any(|kept| kept.name == "monitoring" && kept.why.contains("background task")), "{reaped:?}");
    assert!(reaped.kept.iter().any(|kept| kept.name == "queued" && kept.why.contains("background task")), "a Not done whose CLI still runs the job it waits on is paused, not finished (north#770): {reaped:?}");
    assert!(fleet.session("finished").is_err() && fleet.session("ended").is_err() && fleet.session("monitoring").is_ok());
}

struct Nexus {
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    ended: std::sync::atomic::AtomicBool,
}

impl crate::fleet::jobs::JobLeases for Nexus {
    fn leased(&self, _session: &str, _machine: Option<&crate::fleet::model::Machine>) -> crate::error::NorthResult<bool> {
        let _ = self.release.lock().unwrap().recv();
        self.ended.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(true)
    }
}

#[test]
fn the_reaper_reads_job_leases_from_the_ticks_exchange_and_defers_a_session_whose_lease_read_outlasts_its_budget_instead_of_holding_it_as_leased() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, _paper) = fleet(home.path());
    let (release, held) = std::sync::mpsc::channel();
    let nexus = Arc::new(Nexus { release: std::sync::Mutex::new(held), ended: std::sync::atomic::AtomicBool::new(false) });
    let fleet = fleet.leasing(nexus.clone());
    for name in ["expired", "held", "unexchanged"] {
        reported(&fleet, name, None, "Done: nothing to land.");
    }
    let wait = |session: &str, deadline: &str| crate::fleet::store::WaitRow { session: session.into(), kind: crate::fleet::store::WaitKind::Job, key: format!("{session}#1"), deadline: deadline.into(), created: "2026-11-06T16:00:00Z".into() };
    let waits = [wait("expired", "2026-11-06T17:00:00Z"), wait("held", "2026-11-06T17:03:00Z")];
    fleet.cache_leases(&crate::fleet::store::LeaseTable::read(&waits, ["expired".to_owned(), "held".to_owned()], "2026-11-06T17:01:00Z", now_ms())).unwrap();

    let reaped = reap_within(&fleet, &Sight::default(), 24 * HOUR, now_ms(), true, Duration::from_millis(300), &|| Duration::ZERO);

    assert!(!nexus.ended.load(std::sync::atomic::Ordering::SeqCst), "the reaper waited for the lease read to end");
    drop(release);
    assert_eq!(retired(&reaped), ["expired"], "{reaped:?}");
    let why = |name: &str| reaped.kept.iter().find(|kept| kept.name == name).map(|kept| kept.why.clone()).unwrap_or_default();
    assert!(why("held").contains("holds a lease") && why("unexchanged").contains("next tick"), "{reaped:?}");
    assert_eq!(reaped.unread, 1);
    assert!(reaped.lines().iter().any(|line| line.starts_with("deferred 1 sessions to the next tick: their job leases")), "{:?}", reaped.lines());
    assert!(fleet.session("unexchanged").is_ok() && fleet.session("held").is_ok());
}

// KEEP(bug): on 2026-11-10 at 3:20 PM 27 DeepSeek sessions whose lanes had landed and whose Done reports sat in the proxy's held digest kept their claims on seven issues until forgotten by hand; north#1054
#[test]
fn a_done_session_whose_lane_landed_gives_up_every_claim_and_leaves_the_fleet_in_one_reaper_tick() {
    use crate::fleet::claims::{Claims, Held, Rows};
    let (home, state) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (paper, claims) = (Arc::new(Paper::default()), Arc::new(Held::default()));
    let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), paper.clone(), claims.clone()).unwrap().arming(|_| Ok(()));
    let _lead = live(&fleet, "lead");
    let name = "ds-1041-x";
    reported(&fleet, name, Some("lead"), "Done: the box landed as 43191ff09a on origin/main.");
    paper.claim_receipt(name, "held:1", now_ms()).unwrap();
    claims.hold("north#1041@the box it worked", name, &BTreeSet::new()).unwrap();
    claims.hold("north#1041@a box it claimed itself", name, &BTreeSet::new()).unwrap();
    let stream = fleet.root.join(name).join("g1/stream.jsonl");
    std::fs::File::options().write(true).open(stream).unwrap().set_modified(std::time::UNIX_EPOCH + Duration::from_millis(now_ms() - 600_000)).unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    std::fs::create_dir_all(&queue).unwrap();
    let ticket = queue.join(format!("{}-n1041.json", (now_ms() - 12 * 60_000) * 1000));
    std::fs::write(&ticket, json!({"session": name, "branch": "n1041", "queued_micros": (now_ms() - 12 * 60_000) * 1000}).to_string()).unwrap();
    north_base::land_ticket::Records::of(&ticket).unwrap().moved(&ticket, north_base::land_ticket::Phase::Landed, "landed").unwrap();
    let evidence = Sight { landed: BTreeSet::from(["43191ff09a".to_owned()]), lanes: north_base::land_ticket::lanes_by_session(&state.path().join("north/land-queue")).unwrap(), ..Sight::default() };

    let reaped = reap_after(&fleet, &evidence, 24 * HOUR, now_ms(), true);

    assert_eq!(retired(&reaped), [name], "{reaped:?}");
    assert!(fleet.session(name).is_err(), "a retired session no longer stands parked beside the issue's next box");
    assert_eq!(claims.holders(Rows::Owners(&BTreeSet::from([name.to_owned()]))).unwrap(), Vec::<(String, String)>::new());
}

#[test]
fn a_session_record_that_is_exited_and_quiet_past_the_stale_window_is_retired_and_its_report_stays_readable_through_follow_retired() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, _paper) = fleet(home.path());
    reported(&fleet, "exited", None, "Done: nothing to land.");
    let dir = fleet.root.join("exited");
    let mut record: Value = serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap()).unwrap();
    record["stopped"] = json!(true);
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    assert_eq!(fleet.status(&fleet.session("exited").unwrap()), Status::Exited);

    let inside = reap_after(&fleet, &Sight::default(), 24 * HOUR, now_ms(), true);
    assert!(retired(&inside).is_empty() && fleet.session("exited").is_ok(), "an exited record inside the stale window stays: {inside:?}");
    let past = reap_after(&fleet, &Sight::default(), Duration::ZERO, now_ms(), true);

    assert_eq!(retired(&past), ["exited"], "{past:?}");
    assert!(fleet.session("exited").is_err());
    let report = fleet.follow_retired("exited").unwrap().transcript().entries.iter().any(|entry| entry.text == "Done: nothing to land.");
    assert!(report, "the retired session's report stays readable");
}
