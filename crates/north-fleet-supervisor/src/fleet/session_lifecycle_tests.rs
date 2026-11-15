use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use north_foundation::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Lifecycle, State, Store};
use crate::fleet::session_lifecycle::{Session, store_path};
use serde_json::json;

use north_foundation::error::NorthError;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::limit::{AccountResets, park_limited};
use crate::fleet::model::{Delivery, Phase, Status};
use crate::fleet::records::{read_record, write_record};
use crate::fleet::registry::fixtures::standing_host;
use crate::fleet::registry::{Fleet, Sent};
use north_base::platform::session_host_files::socket_path;
use crate::fleet::runtime::now_ms;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::sender::Sender;
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

/// On 2026-11-14 p-agent-user-slice3's host exited as a zombie its parent never reaped, and a revive took the zombie for a running host and started none.
#[test]
fn a_host_killed_mid_turn_is_an_orphaned_fault_until_the_next_read_parks_its_session_and_a_stored_host_lost_reads_as_parked_and_takes_a_send_and_a_revive_past_a_zombie_host_brings_up_a_host_a_send_reaches() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path()).starting_hosts(standing_host);
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

    let mut store = store;
    store.transition::<Earlier>(&read.launch().instance(), Earlier::HostLost, "a build that had host-lost wrote it", Timestamp::now()).unwrap().unwrap();
    let lost = read_record(&dir).unwrap();
    assert_eq!((lost.phase(), fleet.status(&lost)), (Some(Phase::Parked), Status::Parked));
    let sent = fleet.send_message(&Sender::fleet(), "worker", "are you there", Delivery::Queued);
    assert!(!matches!(&sent, Err(NorthError::Refused(why)) if why.contains("ended")), "{sent:?}");

    let dir = mid_turn(&fleet, "lead");
    let mut zombie = Command::new("cat").stdin(Stdio::piped()).stdout(Stdio::null()).spawn().unwrap();
    let mut held = read_record(&dir).unwrap();
    held.host_pid = Some(zombie.id());
    write_record(&dir, &held).unwrap();
    assert_eq!(fleet.phases().unwrap().live(held.launch(), Some(zombie.id()), "a session host started for it").unwrap(), Phase::Live);
    drop(zombie.stdin.take());
    let pid = rustix::process::Pid::from_raw(zombie.id().cast_signed()).unwrap();
    rustix::process::waitid(rustix::process::WaitId::Pid(pid), rustix::process::WaitidOptions::EXITED | rustix::process::WaitidOptions::NOWAIT).unwrap();

    let revived = fleet.revive("lead", None).unwrap();

    assert_eq!((revived.phase(), revived.host_pid), (Some(Phase::Live), Some(std::process::id())));
    assert!(socket_path(&dir).exists(), "the revive left no socket");
    assert_eq!(fleet.send_message(&Sender::Tom, "lead", "carry on", Delivery::Queued).unwrap(), Sent::HandedIn);
    zombie.wait().unwrap();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Earlier {
    Parked,
    HostLost,
}

impl State for Earlier {
    const ALL: &'static [Self] = &[Self::Parked, Self::HostLost];

    fn name(self) -> &'static str {
        match self {
            Self::Parked => "parked",
            Self::HostLost => "host-lost",
        }
    }
}

impl Lifecycle for Earlier {
    const NAME: &'static str = Session::NAME;
    type State = Self;

    fn legal(_: Option<Self>, _: Self) -> bool {
        true
    }

    fn dwell(_: Self) -> Option<std::time::Duration> {
        None
    }

    fn orphan(_: Self) -> Option<Self> {
        None
    }
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
