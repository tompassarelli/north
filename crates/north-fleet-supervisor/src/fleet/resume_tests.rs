use std::path::{Path, PathBuf};
use std::sync::Mutex;

use std::sync::Arc;

use north_base::land_ticket::{Phase as Ticket, Records, lanes_by_session};

use super::*;
use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::registry::fixtures::live;
use crate::fleet::runtime::now_ms;
use crate::fleet::runtimes::Runtimes;

const MINUTE: u64 = 60_000;

fn queues(state: &Path) -> PathBuf {
    state.join("north/land-queue")
}

fn queued(state: &Path, session: &str, branch: &str, at_ms: u64) -> PathBuf {
    let queue = queues(state).join("de2e0e8479546ba9");
    std::fs::create_dir_all(&queue).unwrap();
    let path = queue.join(format!("{}-{branch}.json", at_ms * 1000));
    std::fs::write(&path, serde_json::json!({"session": session, "branch": branch, "queued_micros": at_ms * 1000}).to_string()).unwrap();
    Records::of(&path).unwrap().moved(&path, Ticket::Queued, "queued").unwrap();
    path
}

fn decided(path: &Path, verdict: &str) {
    Records::of(path).unwrap().moved(path, Ticket::decided(verdict), verdict).unwrap();
}

#[derive(Default)]
struct Recorded {
    revived: Mutex<Vec<String>>,
    attributed: Mutex<Vec<(String, Cause)>>,
}

impl Waker for Recorded {
    fn revive(&self, name: &str) -> NorthResult<()> {
        self.revived.lock().unwrap().push(name.to_owned());
        Ok(())
    }

    fn attribute(&self, name: &str, cause: Cause, _reason: &str) {
        self.attributed.lock().unwrap().push((name.to_owned(), cause));
    }
}

#[test]
fn a_session_that_stopped_before_the_lander_decided_is_woken_though_its_stream_index_is_built_after_by_an_unchanged_tick_from_its_own_record() {
    let (home, state) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()));
    let worker = live(&fleet, "north-790-late");
    let stopped = now_ms() - 10 * MINUTE;
    let stream = fleet.root.join("north-790-late/g1/stream.jsonl");
    let event = serde_json::json!({"type": "assistant", "session_id": "north-790-late", "timestamp": north_foundation::foundation::clock::Timestamp::from_unix_millis(i64::try_from(stopped).unwrap()).iso(), "message": {"content": [{"type": "text", "text": "Stop: landing"}]}});
    std::fs::write(&stream, format!("{event}\n{}\n{}\n", serde_json::json!({"type": "result", "subtype": "success", "session_id": "north-790-late"}), serde_json::json!({"type": "result", "subtype": "success", "session_id": "claimant"}))).unwrap();
    live(&fleet, "claimant");
    let claimant = fleet.root.join("claimant/session.json");
    let mut record: serde_json::Value = serde_json::from_slice(&std::fs::read(&claimant).unwrap()).unwrap();
    record["host_dir"] = fleet.root.join("north-790-late").display().to_string().into();
    std::fs::write(&claimant, record.to_string()).unwrap();
    let lane = queued(state.path(), "north-790-late", "guards", stopped - MINUTE);
    decided(&lane, "failed: the candidate's tests are red");
    fleet.follow_recent("north-790-late").unwrap();
    let held = Held { lanes: lanes_by_session(&queues(state.path())).unwrap(), ..Held::default() };

    let woken = resume(&fleet, &held, &Recorded::default());

    assert_eq!(woken, ["resumed north-790-late: the lander decided its lane"]);
    assert_eq!(worker.frames.lock().unwrap().len(), 1);
    assert!(!fleet.root.join("north-790-late/g1/session-streams/claimant.jsonl").exists(), "indexing a session's own host dir read another session's record");
}
