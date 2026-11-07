use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Store};
use north_session_host::session_lifecycle::{Session, store_path};
use serde_json::json;

use crate::error::NorthError;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::model::{Phase, Status};
use crate::fleet::records::read_record;
use crate::fleet::registry::Fleet;
use crate::fleet::runtimes::Runtimes;

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
    let mut host = Command::new("sleep").arg("60").spawn().unwrap();
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
