use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::handover::{Listening, REPUBLISH_AFTER, release, socket_path};
use super::json::Obj;
use super::offering::{Declines, Waiting, try_offload};
use super::request::{Create, Request};
use super::state::{enqueue, read_queue};
use super::terminations::Terminations;
use crate::capacity::build_offload::{Candidate, Declined, Job};
use crate::capacity::effects::Host;
use crate::capacity::handover::{Frame, Received, receive};

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

fn queued(root: &std::path::Path) -> Vec<String> {
    read_queue(root).iter().filter_map(|ticket| ticket.get("name").and_then(serde_json::Value::as_str).map(str::to_owned)).collect()
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

// KEEP(contract): two peers racing for one run never both take it (the second takes the next run instead of waiting on a claimed one), a run admitted here first withdraws its offer, and a run whose taker failed it is offered again after the wait; north#676
#[tokio::test]
async fn capacity_a_run_is_claimed_once_by_racing_peers_withdrawn_on_local_admission_and_offered_again_after_a_failed_claim() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().to_path_buf();
    let (oldest, newer) = (queue(&root, "heavy", true), queue(&root, "heavy", true));
    let (oldest_door, newer_door) = (Listening::open(&root, &oldest).unwrap(), Listening::open(&root, &newer).unwrap());
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let racers: Vec<_> = (0..2)
        .map(|_| {
            let (root, start) = (root.clone(), std::sync::Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                release(&root)
            })
        })
        .collect();
    let mut claims: Vec<(String, UnixStream)> = racers.into_iter().filter_map(|racer| racer.join().unwrap()).collect();
    claims.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(claims.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), [oldest.as_str(), newer.as_str()], "each racer claims a different run");
    assert!(release(&root).is_none(), "a claimed run is offered to no third peer");
    let _first = oldest_door.accept().await.unwrap();
    let (_, newer_peer) = claims.pop().unwrap();
    drop(newer_door);
    newer_peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let ended = (&newer_peer).read_to_end(&mut Vec::new());
    assert!(matches!(&ended, Ok(0)) || ended.as_ref().is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset), "a run admitted here first ends its claimant's connection instead of leaving it waiting: {ended:?}");
    let later = queue(&root, "heavy", true);
    drop(Listening::open(&root, &later).unwrap());
    assert!(release(&root).is_none(), "a run admitted here first is no longer offered");
    oldest_door.give_back();
    oldest_door.tend_at(Instant::now());
    assert!(release(&root).is_none(), "a failed run waits before it is offered again");
    oldest_door.tend_at(Instant::now() + REPUBLISH_AFTER);
    assert_eq!(release(&root).map(|(name, _)| name), Some(oldest), "a failed run is offered again after the wait");
}

static RECEIVED: Mutex<Vec<u8>> = Mutex::new(Vec::new());

fn relaying_hand_over(_: &Candidate, _: &Job, mut peer: UnixStream, announce: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    let mut output = Vec::new();
    let received = receive(&mut peer, &mut output, &mut std::io::sink(), &mut |host| announce(host));
    RECEIVED.lock().unwrap().extend(output);
    match received {
        Received::Exit(code) => Ok(code),
        Received::NotSynced(why) | Received::Lost(why) => Err(Some(Declined::Failed(why))),
    }
}

fn never_pushed(_: &Candidate, _: &Job, _: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    unreachable!("a pulled run is never pushed")
}

fn host() -> Host {
    Host {
        serving: |_| None,
        warcraft_clients: || None,
        bar1_margin_mib: || None,
        offload: |_| 2,
        build_offload: never_pushed,
        hand_over: relaying_hand_over,
        lease_builds: || true,
        own_build_refusal: || None,
        reclaim_disk: || None,
        tend_disk: || {},
    }
}

fn taker(frames: Vec<Frame>) -> (UnixStream, std::thread::JoinHandle<()>) {
    let (queued, mut peer) = UnixStream::pair().unwrap();
    let feeding = std::thread::spawn(move || {
        for frame in &frames {
            frame.write(&mut peer).unwrap();
        }
    });
    (queued, feeding)
}

async fn hand(root: &std::path::Path, ticket: &str, peer: UnixStream) -> Option<u8> {
    let request = Request { name: "heavy".to_owned(), ..Request::default() };
    let create = Create { kind: "run", owner: "worker".to_owned(), timeout_seconds: Some(60) };
    let command = ["cargo".to_owned(), "test".to_owned()];
    let mut declined = Declines::new();
    let waiting = Waiting { root, ticket: Some(ticket), request: &request, create: &create, command: &command, declined: &mut declined };
    try_offload(waiting, true, &host(), &mut Terminations::none(), Some(peer)).await
}

// KEEP(contract): a waiting run handed to a taker prints the taker's streamed output and exits with the code of its exit frame; one whose taker dies mid-stream keeps its place in the queue and is offered again; north#676
#[tokio::test]
async fn capacity_a_waiting_run_prints_the_takers_stream_and_exits_with_its_code_and_a_taker_that_dies_gives_it_back() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let (first, second) = (queue(root, "heavy", true), queue(root, "heavy", true));
    let recorded = vec![Frame::Started("whiterabbit".to_owned()), Frame::Out(b"running 3 tests\n".to_vec()), Frame::Err(b"warning: unused\n".to_vec()), Frame::Out(b"test result: FAILED\n".to_vec()), Frame::Exit(101)];
    let (peer, feeding) = taker(recorded);
    assert_eq!(hand(root, &first, peer).await, Some(101));
    feeding.join().unwrap();
    assert_eq!(String::from_utf8(std::mem::take(&mut *RECEIVED.lock().unwrap())).unwrap(), "running 3 tests\ntest result: FAILED\n");
    assert_eq!(queued(root), std::slice::from_ref(&second), "a run the taker started leaves the queue");
    let door = Listening::open(root, &second).unwrap();
    let (_, claimed) = release(root).expect("the taker claims the second run");
    drop(claimed);
    let (peer, feeding) = taker(vec![Frame::Started("ccx33".to_owned()), Frame::Out(b"Compiling north".to_vec())]);
    assert_eq!(hand(root, &second, peer).await, None, "a taker that dies before its exit frame decides nothing");
    feeding.join().unwrap();
    assert_eq!(queued(root), std::slice::from_ref(&second), "the run is back in the queue at its own place");
    door.give_back();
    door.tend_at(Instant::now() + REPUBLISH_AFTER);
    assert_eq!(release(root).map(|(name, _)| name), Some(second));
}
