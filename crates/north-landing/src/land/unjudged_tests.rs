use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use super::{Lanes, settle};
use crate::land::heartbeat::Heartbeat;
use crate::land::jobs::Chores;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};
use crate::land::scans::outgoing::denylist::{DenylistVerdict, Stand};

const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const QUEUED_MICROS: u128 = 1_794_396_818_000_000;
const NOW: Duration = Duration::from_micros(1_794_396_819_000_000);

fn lander(dir: &std::path::Path) -> Lander {
    Lander { repo: dir.to_owned(), worktree: dir.to_owned(), queue: dir.join("queue"), checker: Vec::new(), app_token: Vec::new(), offload_owner: String::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default() }
}

fn queued(dir: &std::path::Path, head: &str) -> Lanes {
    let ticket = Ticket { branch: "lane".into(), head: head.into(), unjudged_on: Some(BASE.into()), queued_micros: Some(QUEUED_MICROS), ..Ticket::default() };
    let path = dir.join("queue/lane.json");
    std::fs::create_dir_all(dir.join("queue")).unwrap();
    queue::write(&path, &ticket).unwrap();
    vec![(path, ticket)]
}

#[test]
fn a_lane_is_never_pushed_before_its_private_kind_verdict_whenever_the_verdict_was_asked() {
    let dir = tempfile::tempdir().unwrap();
    let lander = lander(dir.path());
    let asked: Mutex<Vec<(String, Option<Duration>)>> = Mutex::new(Vec::new());
    let passed_heads = Mutex::new(Vec::<String>::new());
    let verdict = |ticket: &Ticket, base: &str, wait: Option<Duration>| {
        assert_eq!(base, BASE, "the judge was asked about another range than the lane's own diff");
        asked.lock().unwrap().push((ticket.head.clone(), wait));
        let stand = if wait.is_some() && passed_heads.lock().unwrap().contains(&ticket.head) { Stand::Pass } else { Stand::Judging };
        DenylistVerdict { lines: Vec::new(), stand }
    };

    let held = settle(&lander, queued(dir.path(), "1111"), NOW, &verdict).unwrap();
    assert!(held.is_empty(), "a lane still being judged was handed to the batch: {held:?}");
    let on_disk = queue::read(&dir.path().join("queue/lane.json")).unwrap();
    assert_eq!((on_disk.unjudged_on.as_deref(), on_disk.alone_green.as_deref()), (Some(BASE), None), "a lane still being judged was vouched for");

    passed_heads.lock().unwrap().push("1111".into());
    asked.lock().unwrap().clear();
    let landed = settle(&lander, queued(dir.path(), "1111"), NOW, &verdict).unwrap();
    assert_eq!(landed.iter().map(|(_, ticket)| (ticket.head.as_str(), ticket.alone_green.as_deref(), ticket.unjudged_on.as_deref())).collect::<Vec<_>>(), [("1111", Some("1111"), None)]);
    let calls = asked.lock().unwrap().clone();
    assert!(matches!(calls.as_slice(), [(_, None), (_, Some(wait))] if *wait > Duration::ZERO), "the round ended instead of waiting for the verdict it had asked for: {calls:?}");

    asked.lock().unwrap().clear();
    let changed = settle(&lander, queued(dir.path(), "2222"), NOW, &verdict).unwrap();
    assert!(changed.is_empty(), "a lane whose diff changed after its verdict was asked landed on the old verdict: {changed:?}");
    let calls = asked.lock().unwrap().clone();
    assert!(!calls.is_empty() && calls.iter().all(|(head, _)| head == "2222"), "the changed diff was not asked about again: {calls:?}");
}
