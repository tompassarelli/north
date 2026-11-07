use std::time::Duration;

use super::{FaultKind, Lifecycle, Liveness, Owner, Refusal, State, Store};
use crate::foundation::clock::Timestamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pull {
    Idle,
    Pulling,
    Abandoned,
}

impl State for Pull {
    const ALL: &'static [Self] = &[Self::Idle, Self::Pulling, Self::Abandoned];

    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pulling => "pulling",
            Self::Abandoned => "abandoned",
        }
    }
}

struct Puller;

impl Lifecycle for Puller {
    const NAME: &'static str = "pull";
    type State = Pull;

    fn legal(from: Option<Pull>, to: Pull) -> bool {
        matches!((from, to), (None | Some(Pull::Idle | Pull::Abandoned), Pull::Pulling) | (Some(Pull::Pulling), Pull::Idle | Pull::Abandoned))
    }

    fn dwell(state: Pull) -> Option<Duration> {
        (state == Pull::Pulling).then_some(Duration::from_secs(80))
    }

    fn orphan(state: Pull) -> Option<Pull> {
        (state == Pull::Pulling).then_some(Pull::Abandoned)
    }
}

struct Dead(u32);

impl Liveness for Dead {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        (owner.host == "laptop").then_some(owner.pid != self.0)
    }
}

fn owner(host: &str, pid: u32) -> Owner {
    Owner { host: host.to_owned(), pid, started: 1, booted_ms: 1 }
}

fn store(path: &std::path::Path, by: Owner, dead: u32) -> Store {
    Store::open_as(path, by, Box::new(Dead(dead))).unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_seconds(1_800_000_000 + seconds)
}

#[test]
fn an_illegal_move_leaves_the_record_unchanged_and_health_sees_it_for_an_hour() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let mut puller = store(&path, owner("laptop", 10), 0);
    puller.transition::<Puller>("laptop", Pull::Pulling, "the snapshot is 15 min old", at(0)).unwrap().unwrap();
    puller.transition::<Puller>("laptop", Pull::Idle, "pulled", at(20)).unwrap().unwrap();

    let refused = puller.transition::<Puller>("laptop", Pull::Abandoned, "a writer that skipped the table", at(30)).unwrap();

    assert_eq!(refused, Err(Refusal::Illegal { from: Some(Pull::Idle), to: Pull::Abandoned }));
    let record = puller.current::<Puller>("laptop").unwrap().unwrap();
    assert_eq!((record.state, record.cause.as_str(), record.seq), (Pull::Idle, "pulled", 2));
    let faults = puller.faults(at(31)).unwrap();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].kind, FaultKind::Illegal { to: "abandoned".into(), at_ms: at(30).unix_millis() });
    assert!(puller.faults(at(30 + 3601)).unwrap().is_empty());
    assert_eq!(puller.history("pull", "laptop").unwrap().iter().filter(|logged| logged.refused).count(), 1);
}

#[test]
fn a_holder_killed_mid_pull_is_reported_and_the_next_taker_logs_its_death_before_taking_over() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    store(&path, owner("laptop", 10), 0).transition::<Puller>("laptop", Pull::Pulling, "the snapshot is 15 min old", at(0)).unwrap().unwrap();

    let mut next = store(&path, owner("laptop", 11), 10);
    let faults = next.faults(at(5)).unwrap();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].kind, FaultKind::Orphaned { owner: owner("laptop", 10) });

    let taken = next.transition::<Puller>("laptop", Pull::Pulling, "the snapshot is 15 min old", at(6)).unwrap().unwrap();
    assert_eq!((taken.state, taken.owner, taken.seq), (Pull::Pulling, Some(owner("laptop", 11)), 3));
    let moves: Vec<_> = next.history("pull", "laptop").unwrap().into_iter().map(|logged| (logged.from, logged.to, logged.cause)).collect();
    assert_eq!(moves[1], (Some("pulling".into()), "abandoned".into(), "the holder, pid 10 on laptop, is gone".into()));
}

#[test]
fn a_live_holder_keeps_its_state_and_one_on_another_host_keeps_it_only_until_its_dwell_runs_out() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    store(&path, owner("laptop", 10), 0).transition::<Puller>("laptop", Pull::Pulling, "pull", at(0)).unwrap().unwrap();
    store(&path, owner("gex45", 20), 0).transition::<Puller>("gex45", Pull::Pulling, "pull", at(0)).unwrap().unwrap();
    let mut other = store(&path, owner("laptop", 11), 0);

    assert_eq!(other.transition::<Puller>("laptop", Pull::Idle, "race", at(5)).unwrap(), Err(Refusal::Held { state: Pull::Pulling, owner: owner("laptop", 10) }));
    assert_eq!(other.transition::<Puller>("gex45", Pull::Idle, "race", at(5)).unwrap(), Err(Refusal::Held { state: Pull::Pulling, owner: owner("gex45", 20) }));
    assert!(other.faults(at(5)).unwrap().is_empty());

    let overdue = other.faults(at(81)).unwrap();
    assert_eq!(overdue.iter().map(|fault| fault.instance.as_str()).collect::<Vec<_>>(), ["laptop", "gex45"]);
    assert!(overdue.iter().all(|fault| matches!(fault.kind, FaultKind::Overdue { .. })));
    assert_eq!(other.transition::<Puller>("gex45", Pull::Pulling, "take over", at(81)).unwrap().unwrap().owner, Some(owner("laptop", 11)));
}

// KEEP(bug): on 2026-11-08 a post-land test that redirected only XDG_STATE_HOME made the lander write 16 red-main moves for a repository named origin into the laptop's real ledger.db, because the host store path read HOME alone (north#844)
#[test]
fn the_host_store_lives_under_the_state_root_a_caller_redirects_and_under_home_only_without_one() {
    let path = |state: Option<&str>, home: Option<&str>| super::store::host_path_in(state.map(Into::into), home.map(Into::into)).ok();
    assert_eq!(path(Some("/scratch/state"), Some("/home/tom")), Some("/scratch/state/north/ledger.db".into()));
    assert_eq!(path(Some(""), Some("/home/tom")), Some("/home/tom/.local/state/north/ledger.db".into()));
    assert_eq!(path(None, Some("/home/tom")), Some("/home/tom/.local/state/north/ledger.db".into()));
    assert_eq!(path(None, None), None);
}
