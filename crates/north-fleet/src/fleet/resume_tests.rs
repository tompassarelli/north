use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use std::sync::Arc;

use north_base::platform::attribution::Harness;
use north_base::land_ticket::{Phase as Ticket, Records, lanes_by_session, lanes_of};

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

fn lanes(state: &Path, session: &str) -> Vec<Lane> {
    lanes_of(&queues(state), session).unwrap()
}

#[test]
fn a_session_waits_on_a_lane_queued_before_it_stopped_until_the_lander_decides_it_after_it_stopped() {
    let state = tempfile::tempdir().unwrap();
    let ended = now_ms() - 5 * MINUTE;
    queued(state.path(), "s", "open", ended - MINUTE);
    let refused = queued(state.path(), "s", "refused", ended - MINUTE);
    let (before, later) = (queued(state.path(), "s", "before", ended - 2 * MINUTE), queued(state.path(), "s", "later", ended + MINUTE));
    for (path, verdict) in [(&refused, "failed: the candidate's tests are red"), (&before, "landed abc1234 on origin/main"), (&later, "landed def5678 on origin/main")] {
        decided(path, verdict);
    }
    let only = |branches: &[&str]| -> Vec<Lane> { lanes(state.path(), "s").into_iter().filter(|lane| branches.contains(&lane.branch.as_str())).collect() };

    assert_eq!(landing(&only(&["open"]), ended), Landing::Waiting);
    assert_eq!(landing(&only(&["refused"]), ended), Landing::Decided);
    assert_eq!(landing(&only(&["refused", "open"]), ended), Landing::Waiting, "one open lane keeps the session waiting");
    assert_eq!(landing(&only(&["before"]), now_ms() + MINUTE), Landing::Settled, "decided before the stop, the session knew it");
    assert_eq!(landing(&only(&["later"]), ended), Landing::Settled, "a lane queued after the stop is not the one it stopped on");
    assert_eq!(landing(&[], ended), Landing::Settled);
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

fn stopped_at(fleet: &Fleet, name: &str, ago: Duration) {
    let stream = fleet.root.join(name).join("g1/stream.jsonl");
    File::options().write(true).open(stream).unwrap().set_modified(SystemTime::now() - ago).unwrap();
}

#[test]
fn a_session_stopped_on_its_queued_lane_is_woken_once_when_the_lander_decides_it() {
    let (home, state) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()));
    let worker = live(&fleet, "north-669-d1955");
    stopped_at(&fleet, "north-669-d1955", Duration::from_secs(600));
    let lane = queued(state.path(), "north-669-d1955", "guards", now_ms() - 12 * MINUTE);
    let waker = Recorded::default();

    let by_session = |state: &Path| lanes_by_session(&queues(state)).unwrap();
    assert!(resume(&fleet, &by_session(state.path()), &waker).is_empty(), "an open lane wakes nothing");
    decided(&lane, "failed: the candidate's tests are red");

    let first = resume(&fleet, &by_session(state.path()), &waker);
    let sent = worker.frames.lock().unwrap().clone();
    let mut stream = File::options().append(true).open(fleet.root.join("north-669-d1955/g1/stream.jsonl")).unwrap();
    writeln!(stream, "{}", serde_json::json!({"type": "user", "session_id": "north-669-d1955", "message": {"content": [{"type": "text", "text": format!("[from service:north-fleet] {}", message(&lanes(state.path(), "north-669-d1955"), 0))}]}})).unwrap();
    stopped_at(&fleet, "north-669-d1955", Duration::from_secs(600));
    let again = resume(&fleet, &by_session(state.path()), &waker);

    assert_eq!(first, ["resumed north-669-d1955: the lander decided its lane"]);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains(OPENER) && sent[0].contains("guards: failed: the candidate's tests are red"), "{sent:?}");
    assert!(again.is_empty() && worker.frames.lock().unwrap().len() == 1, "one stop wakes once: {again:?}");
    assert_eq!(*waker.attributed.lock().unwrap(), [("north-669-d1955".to_owned(), Cause::Harness(Harness::LandingRefusal))]);
    assert!(waker.revived.lock().unwrap().is_empty(), "a session whose host runs needs no revive");
}
