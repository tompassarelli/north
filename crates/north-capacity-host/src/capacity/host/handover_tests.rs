use std::os::unix::net::UnixListener;
use std::time::Duration;

use super::handover::{Listening, release, socket_path};
use super::json::Obj;
use super::state::enqueue;

fn queue(root: &std::path::Path, class: &str, pullable: bool) -> String {
    std::thread::sleep(Duration::from_millis(3));
    enqueue(root, Obj::new().put("class", class).put("requestedClass", class).put("requestedCpus", 6).put_some("pullable", pullable.then_some(true)))
}

fn door(root: &std::path::Path, ticket: &str) -> UnixListener {
    let path = socket_path(root, ticket);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    UnixListener::bind(path).unwrap()
}

fn connected(listener: &UnixListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    listener.accept().is_ok()
}

// KEEP(contract): the hand-over releases only the oldest queued agent heavy run that opened a door, to one caller, and a run closes its door when it leaves the queue; north#634
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
    let oldest_door = Listening::open(root, &oldest).expect("the oldest run opens its door");
    let _ = shut;
    let (released, stream) = release(root).expect("a queued agent heavy run");
    assert_eq!(released, oldest);
    let accepted = oldest_door.accept().await.expect("the run sees the caller");
    assert!(!connected(&newer_door) && !connected(&moderate_door) && !connected(&local_door), "only the oldest run is released");
    drop((stream, accepted, oldest_door));
    assert!(!socket_path(root, &oldest).exists(), "a run that leaves the queue closes its door");
    assert_eq!(release(root).map(|(name, _)| name), Some(newer.clone()), "the next run in line is released next");
}
