use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Store};
use crate::fleet::session_lifecycle::{Session, store_path};
use serde_json::json;

use crate::error::NorthError;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::limit::{AccountResets, park_limited};
use crate::fleet::model::{Phase, Status};
use crate::fleet::records::read_record;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::waits::Run;

fn fleet(home: &Path) -> Fleet {
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()))
}

fn mid_turn(fleet: &Fleet, name: &str) -> PathBuf {
    let dir = fleet.root.join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": name, "host_dir": dir, "stream_offset": 0});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let asked = json!({"type": "user", "session_id": name, "message": {"content": [{"type": "text", "text": "Item: north#757"}]}});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{asked}\n")).unwrap();
    dir
}

#[test]
fn a_retired_session_refuses_a_revival_and_keeps_its_retired_record_with_the_reports_stop_line() {
    fleet_session_model_is_sound();
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    mid_turn(&fleet, "worker");
    let archived = fleet.retire("worker", "reported and parentless", Some("Not done: main is red.\nStop: landing")).unwrap();
    let record = read_record(&archived).unwrap();

    let revived = fleet.phases().unwrap().live(record.launch(), Some(std::process::id()), "a message revived it");

    assert!(matches!(&revived, Err(NorthError::Refused(why)) if why.contains("from retired to live")), "{revived:?}");
    let store = Store::open(&store_path(&fleet.root)).unwrap();
    let kept = store.current::<Session>(&record.launch().instance()).unwrap().unwrap();
    assert_eq!((kept.state, kept.cause.as_str()), (Phase::Retired, "reported and parentless; Stop: landing"));
    let faults = store.faults(Timestamp::now()).unwrap();
    assert!(faults.iter().any(|fault| matches!(&fault.kind, FaultKind::Illegal { to, .. } if to == "live")), "{faults:?}");
}

#[test]
fn a_host_killed_mid_turn_is_an_orphaned_fault_until_the_next_read_parks_its_session() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let dir = mid_turn(&fleet, "worker");
    let mut host = Command::new("cat").stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
    let started = read_record(&dir).unwrap();
    assert_eq!(fleet.phases().unwrap().live(started.launch(), Some(host.id()), "a session host started for it").unwrap(), Phase::Live);

    host.kill().unwrap();
    host.wait().unwrap();

    let store = Store::open(&store_path(&fleet.root)).unwrap();
    let faults = store.faults(Timestamp::now()).unwrap();
    assert!(matches!(faults.as_slice(), [fault] if fault.instance == "worker@1" && matches!(&fault.kind, FaultKind::Orphaned { owner } if owner.pid == host.id())), "{faults:?}");
    let read = read_record(&dir).unwrap();
    assert_eq!((read.phase(), fleet.status(&read)), (Some(Phase::Parked), Status::Parked));
    assert!(store.faults(Timestamp::now()).unwrap().is_empty());
}

#[test]
fn a_session_launched_without_a_task_holds_the_issue_it_files_in_its_record_north_817() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let dir = mid_turn(&fleet, "north-ship-slot-race");

    fleet.hold_task("north-ship-slot-race", "north#815").unwrap();
    fleet.hold_task("north-ship-slot-race", "north#815").unwrap();
    fleet.hold_task("north-ship-slot-race", "north#816").unwrap();

    let record = read_record(&dir).unwrap();
    assert_eq!(record.tasks().collect::<Vec<_>>(), ["north#815", "north#816"], "dispatch staffed north-815-d1556 beside the session that filed north#815");
}

#[test]
fn the_reports_mirror_names_the_newest_retired_session_of_each_task_however_old_north_706() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    for (name, task, created) in [("wisp-188-d1646", "wisp#188", 1), ("wisp-188-d1700", "wisp#188", 2), ("wisp-87-d0221", "wisp#87", 1)] {
        let dir = mid_turn(&fleet, name);
        let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": created, "host_pid": null, "conversation": name, "host_dir": dir, "stream_offset": 0, "task_ref": task});
        std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
        fleet.forget(name).unwrap();
    }

    assert_eq!(fleet.retired_names_read(u64::MAX), ["wisp-188-d1700", "wisp-87-d0221"], "dispatch noted wisp-188-d1646 report unread days after it retired");
    assert_eq!(fleet.retired_names_read(0).len(), 3, "everything archived since the cut-off is named too");
}

struct NoResets;

impl AccountResets for NoResets {
    fn reset_ms(&self, _: &str, _: u64) -> Option<u64> {
        None
    }
}

fn append(dir: &Path, frames: &[serde_json::Value]) {
    use std::io::Write;
    let mut stream = std::fs::OpenOptions::new().append(true).open(dir.join("g1/stream.jsonl")).unwrap();
    for frame in frames {
        writeln!(stream, "{frame}").unwrap();
    }
}

#[test]
fn a_host_that_ends_on_the_week_limit_in_the_middle_of_a_command_leaves_its_session_parked_with_a_usage_wait_north_1013() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let dir = mid_turn(&fleet, "worker");
    let week = now_ms() / 1000 + 3 * 86_400;
    append(&dir, &[
        json!({"type": "assistant", "session_id": "worker", "message": {"content": [{"type": "text", "text": "Running the build."}]}}),
        json!({"type": "rate_limit_event", "session_id": "worker", "rate_limit_info": {"status": "rejected", "resetsAt": week}}),
    ]);

    let parked = park_limited(&fleet, &NoResets, now_ms());

    assert_eq!(parked.iter().map(|parked| parked.session.as_str()).collect::<Vec<_>>(), ["worker"]);
    let waits = fleet.waits().of("worker");
    assert!(matches!(waits.as_slice(), [wait] if matches!(&wait.run, Run::Usage { account, at_ms } if account == "a" && *at_ms == week * 1000)), "{waits:?}");
    assert_eq!(read_record(&dir).unwrap().phase(), Some(Phase::Parked));
}

#[test]
fn a_retired_session_with_no_done_report_reopens_as_a_new_launch_of_its_own_conversation_north_1013() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let dir = mid_turn(&fleet, "worker");
    fleet.retire("worker", "a launch took over its name", None).unwrap();

    let reopened = fleet.reopened("worker").unwrap();

    assert_eq!((reopened.phase(), reopened.conversation.as_deref(), reopened.addressed_since()), (Some(Phase::Parked), Some("worker"), 1));
    assert!(reopened.created_ms > 1);
    assert!(dir.join("session.json").exists());
}

/// The fleet-session model `north check machines` explores: one instance through every ordering, then a second
/// instance starting at every situation the name claim admits, and two launches racing from no record. The assertion
/// is here so `cargo nextest run -p north-fleet-supervisor -E 'test(a_retired_session_refuses_a_revival_and_keeps_its_retired_record_with_the_reports_stop_line)'`
/// replays the model, and a launch that stops claiming the name atomically fails it.
fn fleet_session_model_is_sound() {
    use north_machine_model::{Promises, Together, explore_together};
    use crate::fleet::session_lifecycle::claim;
    let together = Together {
        alone: &[Phase::Live, Phase::Parked, Phase::IdleExited, Phase::HostEnded, Phase::Stopped, Phase::Adopted],
        admits: |other| match other {
            Phase::Retired => true,
            _ => tempfile::tempdir().is_ok_and(|root| {
                let path = root.path().join("launch");
                claim(&path).is_ok() && claim(&path).is_ok()
            }),
        },
        race: || {
            let Ok(root) = tempfile::tempdir() else { return 0 };
            let path = root.path().join("launch");
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..2).map(|_| scope.spawn(|| claim(&path).is_ok())).collect();
                handles.into_iter().map(|handle| handle.join().unwrap_or(false)).filter(|won| *won).count()
            })
        },
    };
    let model = explore_together::<Session>(&Promises { ends: &[Phase::Retired], repeats: &[Phase::Live, Phase::Stopped, Phase::Adopted] }, &together).expect("the fleet-session search runs");
    assert_eq!(model.instances, 2, "the name claim is checked between two launches");
    let broken: Vec<String> = model.findings.iter().map(|finding| format!("{}: {}", model.machine, finding.line())).collect();
    assert!(broken.is_empty(), "{}", broken.join("; "));
}

#[test]
fn a_stop_that_met_a_lost_race_to_a_retirement_is_done_and_leaves_no_illegal_move() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let record = read_record(&mid_turn(&fleet, "worker")).unwrap();
    let mut stopper = fleet.phases().unwrap();
    let seen = Some(stopper.of(record.launch(), &json!({})).unwrap());
    let mut retirer = fleet.phases().unwrap();
    retirer.moved(record.launch(), Phase::Stopped, "a launch took its name over").unwrap();
    retirer.moved(record.launch(), Phase::Retired, "archived").unwrap();

    stopper.stopped_from(record.launch(), seen, "north fleet stop ended the turn").unwrap();
    fleet.stop("worker").unwrap();

    let store = Store::open(&store_path(&fleet.root)).unwrap();
    assert_eq!(store.current::<Session>(&record.launch().instance()).unwrap().unwrap().state, Phase::Retired);
    let moves = store.history("fleet-session", &record.launch().instance()).unwrap();
    assert!(moves.iter().all(|logged| !logged.refused), "{moves:?}");
    assert!(store.faults(Timestamp::now()).unwrap().is_empty());
}
