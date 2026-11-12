use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use super::{dialed_through, read};
use crate::land::heartbeat::Heartbeat;
use crate::land::jobs::Chores;
use crate::land::lander::Lander;
use crate::land::queue::{self, HandIn, Ticket};
use crate::land::scans::outgoing::denylist::{DenylistVerdict, Stand};
use crate::land::unjudged::{Asks, Lanes, Verdict};

const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const QUEUED_MICROS: u128 = 1_794_396_818_000_000;
const SOON: Duration = Duration::from_micros(1_794_396_819_000_000);
const LATE: Duration = Duration::from_micros(1_794_397_818_000_000);
const REFUSAL: &str = "privacy: file it in south: person (\"a name\")\nsafe-push: ✗ PRIVATE KIND for a PUBLIC repository, the outgoing messages or docs name private detail; NOT pushed.\n";

fn answered(code: i32, stdout: &str) -> std::io::Result<Output> {
    Ok(Output { status: ExitStatus::from_raw(code << 8), stdout: stdout.as_bytes().to_vec(), stderr: b"ssh: connect to host worker-host port 22: Connection refused\n".to_vec() })
}

fn lander(dir: &Path) -> Lander {
    Lander { repo: dir.to_owned(), worktree: dir.to_owned(), queue: dir.join("queue"), checker: Vec::new(), app_token: Vec::new(), offload_owner: String::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default(), wake: std::sync::Arc::default() }
}

fn handed_in(dir: &Path, branch: &str) -> Lanes {
    let hand_in = HandIn { called_micros: QUEUED_MICROS - 900_000, gates_micros: QUEUED_MICROS - 100_000, asked_micros: QUEUED_MICROS - 400_000, verdict_micros: None, wakes_session: false, judge: None, check: None };
    let ticket = Ticket { branch: branch.into(), host: "worker-host".into(), head: format!("{branch}-head"), unjudged_on: Some(BASE.into()), queued_micros: Some(QUEUED_MICROS), hand_in: Some(hand_in), ..Ticket::default() };
    let path = dir.join(format!("queue/{branch}.json"));
    std::fs::create_dir_all(dir.join("queue")).unwrap();
    queue::write(&path, &ticket).unwrap();
    vec![(path, ticket)]
}

fn from_host(code: i32) -> impl Fn(&Ticket, &str, Option<Duration>) -> DenylistVerdict + Send + Sync + 'static {
    move |ticket, base, _| {
        assert_eq!((ticket.head.as_str(), base), ("lane-head", BASE), "the worker's host was asked about another text than the lane's own head");
        read("worker-host", answered(code, ""))
    }
}

fn taken(lander: &Lander, tickets: Lanes, now: Duration, verdict: &Arc<Verdict>) -> Lanes {
    let mut asks = Asks::default();
    let asking = asks.settle(lander, tickets.clone(), now, verdict).unwrap();
    assert!(asking.is_empty(), "a lane was handed to the batch before its verdict was answered: {asking:?}");
    assert!(north_base::platform::wait::until(Duration::from_secs(60), || asks.running() == 0), "the verdict ask never ended");
    asks.settle(lander, tickets, now, verdict).unwrap()
}

#[test]
fn a_lane_handed_in_on_another_host_is_vouched_for_only_by_that_hosts_passing_verdict_on_its_own_head() {
    let refused = read("worker-host", answered(1, REFUSAL));
    assert_eq!((refused.stand, refused.lines.join("\n") + "\n"), (Stand::Refuse, REFUSAL.to_owned()), "a refusal read from the worker's host lost the judge's own words");

    assert_eq!((dialed_through("cpx62.lan", |named| Ok(format!("agent@{named}"))), dialed_through("cpx62.lan", |named| Ok(named.to_owned()))), (Ok("agent@cpx62".to_owned()), Ok("cpx62.lan".to_owned())),"the verdict ask did not follow the host's agent login, or changed the host it dials when none is named");

    let dir = tempfile::tempdir().unwrap();
    let lander = lander(dir.path());
    for (case, code, now) in [("unreachable", 255, SOON), ("still judging", 3, SOON), ("unreachable past the lander's patience", 255, LATE), ("still judging past the lander's patience", 3, LATE), ("unreadable", 2, SOON)] {
        let settled = taken(&lander, handed_in(dir.path(), "lane"), now, &(Arc::new(from_host(code)) as Arc<Verdict>));
        let on_disk = queue::read(&dir.path().join("queue/lane.json")).unwrap();
        assert!(settled.iter().chain([&(PathBuf::new(), on_disk.clone())]).all(|(_, ticket)| !ticket.passed_alone() && ticket.verdict.is_none()), "a lane whose verdict was {case} was vouched for or decided: {on_disk:?}");
        if now == SOON && code != 2 {
            assert!(settled.is_empty() && on_disk.unjudged_on.as_deref() == Some(BASE), "a lane whose verdict was {case} stopped waiting for it: {on_disk:?}");
        }
    }

    let mut both = handed_in(dir.path(), "lane");
    both.extend(handed_in(dir.path(), "other"));
    let only_lane_passes = |ticket: &Ticket, base: &str, wait: Option<Duration>| if ticket.branch == "lane" { from_host(0)(ticket, base, wait) } else { read("worker-host", answered(3, "")) };
    let landed = taken(&lander, both, SOON, &(Arc::new(only_lane_passes) as Arc<Verdict>));
    let vouched: Vec<_> = landed.iter().map(|(_, ticket)| (ticket.branch.as_str(), ticket.passed_alone(), ticket.hand_in.as_ref().and_then(|hand_in| hand_in.verdict_micros))).collect();
    assert_eq!(vouched, [("lane", true, Some(SOON.as_micros()))], "the lane with its passing verdict did not join the batch on its own, or its verdict time went unrecorded, while another lane still waited");
}
