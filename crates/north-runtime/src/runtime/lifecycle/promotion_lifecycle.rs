use super::{Promoting, Promotion};
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Liveness, Owner, Refusal, Store};

const SHA: &str = "2222222222222222222222222222222222222222";

struct Dead(u32);

impl Liveness for Dead {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        (owner.host == "laptop").then_some(owner.pid != self.0)
    }
}

fn owner(pid: u32) -> Owner {
    Owner { host: "laptop".to_owned(), pid, started: 1, booted_ms: 1 }
}

fn store(path: &std::path::Path, pid: u32, dead: u32) -> Store {
    Store::open_as(path, owner(pid), Box::new(Dead(dead))).unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000 + seconds)
}

#[test]
fn a_failed_promotion_cannot_go_live_without_installing_again_and_health_sees_the_attempt() {
    let directory = tempfile::tempdir().unwrap();
    let mut promoting = store(&directory.path().join("ledger.db"), 10, 0);
    promoting.transition::<Promoting>(SHA, Promotion::Installing, "built", at(0)).unwrap().unwrap();
    promoting.transition::<Promoting>(SHA, Promotion::InstallFailed, "failed step health check", at(30)).unwrap().unwrap();

    let refused = promoting.transition::<Promoting>(SHA, Promotion::Live, "a writer that skipped the check", at(40)).unwrap();

    assert_eq!(refused, Err(Refusal::Illegal { from: Some(Promotion::InstallFailed), to: Promotion::Live }));
    let record = promoting.current::<Promoting>(SHA).unwrap().unwrap();
    assert_eq!((record.state, record.cause.as_str()), (Promotion::InstallFailed, "failed step health check"));
    let faults = promoting.faults(at(41)).unwrap();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].kind, FaultKind::Illegal { to: "live".into(), at_ms: at(40).unix_millis() });
}

#[test]
fn a_promotion_killed_between_the_swap_and_its_verdict_is_reported_and_the_next_pass_logs_it_before_retrying() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    store(&path, 10, 0).transition::<Promoting>(SHA, Promotion::Installing, "built; swapping current to it", at(0)).unwrap().unwrap();

    let mut next = store(&path, 11, 10);
    let faults = next.faults(at(5)).unwrap();
    assert_eq!((faults.len(), &faults[0].kind, faults[0].state.as_str()), (1, &FaultKind::Orphaned { owner: owner(10) }, "installing"));

    let retried = next.transition::<Promoting>(SHA, Promotion::Installing, "retrying the unfinished promotion", at(6)).unwrap().unwrap();
    assert_eq!((retried.state, retried.owner, retried.seq), (Promotion::Installing, Some(owner(11)), 3));
    let moves = next.history("runtime-promotion", SHA).unwrap();
    assert_eq!((moves[1].from.as_deref(), moves[1].to.as_str(), moves[1].cause.as_str()), (Some("installing"), "unfinished", "the holder, pid 10 on laptop, is gone"));
}
