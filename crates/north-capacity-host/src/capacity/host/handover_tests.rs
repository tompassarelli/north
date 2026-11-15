use std::os::unix::net::UnixListener;
use std::time::Duration;

use north_base::platform::wait;

const HANG: Duration = Duration::from_secs(3600);

use super::handover::{Listening, release, socket_path};
use super::json::Obj;
use super::state::enqueue;

fn enqueue_later(root: &std::path::Path, ticket: Obj) -> String {
    let mark = super::machine::now_ms();
    wait::until(HANG, || super::machine::now_ms() > mark);
    enqueue(root, ticket)
}

fn queue(root: &std::path::Path, class: &str, pullable: bool) -> String {
    enqueue_later(root, Obj::new().put("class", class).put("requestedClass", class).put("requestedCpus", 6).put_some("pullable", pullable.then_some(true)))
}

fn door(root: &std::path::Path, ticket: &str) -> UnixListener {
    let path = socket_path(root, ticket);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    UnixListener::bind(path).unwrap()
}

const OPERATOR: &str = "0::/user.slice/user-1000.slice/session-4.scope\n";
const WORKER: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-session-77c49ab1.scope\n";

fn connected(listener: &UnixListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    listener.accept().is_ok()
}

#[tokio::test]
async fn capacity_the_hand_over_verb_releases_the_oldest_queued_agent_heavy_run() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    assert!(release(root).is_none(), "nothing is queued");
    let moderate = queue(root, "moderate", true);
    let shut = queue(root, "heavy", true);
    let local = queue(root, "heavy", false);
    let oldest = queue(root, "heavy", true);
    let newer = queue(root, "heavy", true);
    let (moderate_door, local_door, newer_door) = (door(root, &moderate), door(root, &local), door(root, &newer));
    let oldest_door = Listening::open_reading(root, &oldest, |_| Some(OPERATOR.to_owned())).expect("the oldest run opens its door");
    let _ = shut;
    let (released, stream) = release(root).expect("a queued agent heavy run");
    assert_eq!(released, oldest);
    let accepted = oldest_door.accept().await.expect("the run sees the caller");
    assert!(!connected(&newer_door) && !connected(&moderate_door) && !connected(&local_door), "only the oldest run is released");
    drop((stream, accepted, oldest_door));
    assert!(!socket_path(root, &oldest).exists(), "a run that leaves the queue closes its door");
    assert_eq!(release(root).map(|(name, _)| name), Some(newer.clone()), "the next run in line is released next");
}

#[tokio::test]
async fn capacity_a_queued_runs_door_refuses_a_worker_scope_peer() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let ticket = queue(root, "heavy", true);
    let door = Listening::open_reading(root, &ticket, |_| Some(WORKER.to_owned())).expect("the run opens its door");
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path(root, &ticket)).unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(500), door.accept()).await.is_err(), "a peer in a worker's scope takes no run");
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(std::io::Read::read(&mut stream, &mut [0u8; 1]).unwrap(), 0, "the door closes a worker-scope peer's connection");
}
