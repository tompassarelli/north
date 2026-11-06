use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::gates::{GateBook, GateRequest};
use crate::fleet::model::{Delivery, Ending};
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
}

impl Evidence for Sight {
    fn landed(&self, _cwd: &Path, sha: &str) -> bool {
        self.landed.contains(sha)
    }

    fn running(&self, records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        records.iter().filter_map(|record| Some((record.name.clone(), *self.running.get(&record.name)?))).collect()
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
    assert!(record.stopped && record.ended == Some(Ending::Retired), "{record:?}");
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

#[test]
fn a_message_to_a_retired_session_reaches_its_parent_as_one_turn_and_never_revives_it() {
    let home = tempfile::tempdir().unwrap();
    let (fleet, _paper) = fleet(home.path());
    let lead = live(&fleet, "lead");
    reported(&fleet, "worker", Some("lead"), "Done: nothing to land.");
    fleet.retire("worker", "test", Some("Done: nothing to land.")).unwrap();

    let sent = fleet.send_message(&Sender::Tom, "worker", "are you there", Delivery::Queued).unwrap();
    let row = FleetRow { id: format!("{}-{}", now_ms(), new_uuid().unwrap()), kind: RowKind::Message, to: "worker".into(), from: "far".into(), urgent: false, sent_at: now_ms(), text: "from the far host".into(), sender: Sender::Session("peer".into()), gate: None, answer: None, job: None, origin: None };
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
        let gate = GateRequest { verification: false, issue: None, box_text: None, session: "gated".into(), question: "Which?".into(), options: vec!["a".into(), "b".into()], recommend: 1, default: 1, default_after_ms: 0 };
        paper.open_gate(&gate, now_ms()).unwrap();
        let queue = home.path().join(".local/state/north/land-queue/560e3f31dbaa2415");
        std::fs::create_dir_all(&queue).unwrap();
        std::fs::write(queue.join("1-queued.json"), json!({"root": "/elsewhere", "branch": "queued-lane", "session": "queued"}).to_string()).unwrap();
        let evidence = Sight { running: [("running".to_owned(), Work::Detached)].into(), ..Sight::default() };
    
        let spent = reap_within(&fleet, &evidence, Duration::ZERO, now_ms(), true, Duration::ZERO);
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
    reported(&fleet, "ended", Some("lead"), "Done: the arm result landed as 2bc8279e0.");
    for name in ["finished", "monitoring", "ended"] {
        paper.claim_receipt(name, "turn:1", now_ms()).unwrap();
    }
    let dir = fleet.root.join("ended");
    let mut record = crate::fleet::records::read_record(&dir).unwrap();
    record.stopped = true;
    record.ended = Some(Ending::IdleExit);
    crate::fleet::records::write_record(&dir, &record).unwrap();
    fleet.waits().register("ended", Run::Process { pid: 1, started: 1 }, Path::new("/tmp"), Some("sh -c d=$0 u=$1 limit=$2")).unwrap();
    let running = ["finished", "monitoring"].map(|name| (name.to_owned(), Work::Cli)).into();
    let evidence = Sight { landed: ["1af7168df".to_owned()].into(), running };

    let reaped = reap_after(&fleet, &evidence, Duration::ZERO, now_ms(), true);

    let mut names = retired(&reaped);
    names.sort_unstable();
    assert_eq!(names, ["ended", "finished"], "{reaped:?}");
    assert!(reaped.kept.iter().any(|kept| kept.name == "monitoring" && kept.why.contains("background task")), "{reaped:?}");
    assert!(fleet.session("finished").is_err() && fleet.session("ended").is_err() && fleet.session("monitoring").is_ok());
}
