use std::time::Duration;

use super::{Delivery, Effect, FaultKind, Leased, Lifecycle, Liveness, Owner, Refusal, Stage, State, Store};
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

#[test]
fn the_host_store_lives_under_the_state_root_a_caller_redirects_and_under_home_only_without_one() {
    let path = |state: Option<&str>, home: Option<&str>| super::store::host_path_in(state.map(Into::into), home.map(Into::into)).ok();
    assert_eq!(path(Some("/scratch/state"), Some("/home/tom")), Some("/scratch/state/north/ledger.db".into()));
    assert_eq!(path(Some(""), Some("/home/tom")), Some("/home/tom/.local/state/north/ledger.db".into()));
    assert_eq!(path(None, Some("/home/tom")), Some("/home/tom/.local/state/north/ledger.db".into()));
    assert_eq!(path(None, None), None);
}

const CRASH: &str = "NORTH_OUTBOX_CRASH";
const SENT: &str = "outbox-sent";
const POINTS: [&str; 3] = ["committed", "leased", "sent"];

fn effects(point: &str) -> [Effect; 2] {
    let effect = |delivery, name: &str| Effect { kind: "pull-started".into(), key: format!("{point}/{name}"), payload: serde_json::json!({ "point": point }), delivery };
    [effect(Delivery::ReplaySafe, "replay-safe"), effect(Delivery::ProcessBound, "process-bound")]
}

fn send_once(sink: &std::path::Path, leased: &Leased) -> Result<(), String> {
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(sink.join(SENT)).map_err(|error| error.to_string())?;
    std::io::Write::write_all(&mut log, format!("{}\n", leased.effect.key).as_bytes()).map_err(|error| error.to_string())?;
    match std::fs::File::create_new(sink.join(leased.effect.key.replace('/', "-"))) {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => Err(error.to_string()),
        _ => Ok(()),
    }
}

fn crash_at(point: &str, root: &std::path::Path) -> ! {
    let mut store = Store::open(&root.join("ledger.db")).unwrap();
    store.transition_with_effects::<Puller>(point, Pull::Pulling, "started", at(0), None, &effects(point)).unwrap().unwrap();
    while point != "committed"
        && let Some(leased) = store.lease("pull-started", at(1), Duration::from_secs(600)).unwrap()
    {
        if point == "sent" {
            send_once(root, &leased).unwrap();
        }
    }
    rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::Kill).unwrap();
    unreachable!("a process killed with SIGKILL runs no further")
}

#[test]
fn a_process_killed_between_a_move_and_its_effects_leaves_each_replay_safe_effect_delivered_once_and_each_process_bound_one_retired() {
    if let Some(crash) = std::env::var_os(CRASH) {
        let (point, root) = crash.to_str().unwrap().split_once(':').unwrap();
        crash_at(point, std::path::Path::new(root));
    }
    let directory = tempfile::tempdir().unwrap();
    for point in POINTS {
        let killed = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lifecycle::tests::a_process_killed_between_a_move_and_its_effects_leaves_each_replay_safe_effect_delivered_once_and_each_process_bound_one_retired", "--nocapture"])
            .env(CRASH, format!("{point}:{}", directory.path().display()))
            .output()
            .unwrap();
        assert!(!killed.status.success(), "{point}: {}", String::from_utf8_lossy(&killed.stderr));
    }

    let mut restarted = Store::open(&directory.path().join("ledger.db")).unwrap();
    let mut received = Vec::new();
    let delivered = restarted
        .deliver("pull-started", at(2), Duration::from_secs(600), |leased| {
            received.push(leased.effect.key.clone());
            send_once(directory.path(), leased)
        })
        .unwrap();

    assert_eq!((delivered, received), (3, POINTS.map(|point| format!("{point}/replay-safe")).to_vec()));
    for point in POINTS {
        let [replay_safe, process_bound] = effects(point);
        assert_eq!(restarted.stage(&replay_safe.key).unwrap(), Some(Stage::Delivered), "{point}");
        assert!(directory.path().join(replay_safe.key.replace('/', "-")).exists(), "{point}");
        assert_eq!(restarted.stage(&process_bound.key).unwrap(), Some(Stage::Retired), "{point}");
    }
    assert_eq!(restarted.lease("pull-started", at(3), Duration::from_secs(600)).unwrap(), None);
    let refused = restarted.transition_with_effects::<Puller>("fresh", Pull::Idle, "an illegal first move", at(4), None, &effects("refused")).unwrap();
    assert!(refused.is_err());
    assert_eq!(restarted.stage("refused/replay-safe").unwrap(), None);
}

#[test]
fn an_effect_that_failed_every_delivery_and_one_leased_past_its_hold_are_health_faults_of_the_instance_that_started_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = store(&directory.path().join("ledger.db"), owner("laptop", 10), 0);
    store.transition_with_effects::<Puller>("laptop", Pull::Pulling, "started", at(0), None, &effects("planted")).unwrap().unwrap();
    for attempt in 0..5 {
        let leased = store.lease("pull-started", at(attempt * 60), Duration::from_secs(30)).unwrap().unwrap();
        assert_eq!(leased.effect.key, "planted/replay-safe");
        store.undelivered(&leased.effect.key, "the receiver is down", at(attempt * 60)).unwrap();
    }
    let held = store.lease("pull-started", at(300), Duration::from_secs(30)).unwrap().unwrap();
    assert_eq!(held.effect.key, "planted/process-bound");

    let outbox: Vec<_> = store.faults(at(400)).unwrap().into_iter().filter(|fault| fault.state == "pull-started").collect();

    let kinds: Vec<_> = outbox.iter().map(|fault| fault.kind.clone()).collect();
    assert_eq!(kinds, [FaultKind::Undeliverable { key: "planted/replay-safe".into(), attempts: 5 }, FaultKind::LeaseLapsed { key: "planted/process-bound".into(), lease_until_ms: at(330).unix_millis() }]);
    assert_eq!(outbox[0].line(at(400)), "pull/laptop pull-started: its effect planted/replay-safe failed all 5 deliveries in 6 min: the receiver is down");
    assert_eq!(outbox[1].line(at(400)), "pull/laptop pull-started: its effect planted/process-bound is still leased 1 min past its hold, 6 min after its move");
    assert!(store.faults(at(320)).unwrap().iter().all(|fault| !matches!(fault.kind, FaultKind::LeaseLapsed { .. })));
}

#[test]
fn a_gone_holder_s_row_is_retired_and_reported_once() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    store(&path, owner("laptop", 10), 0).transition::<Puller>("gex45", Pull::Pulling, "pulling", at(0)).unwrap().unwrap();
    let mut next = store(&path, owner("laptop", 20), 10);
    assert_eq!(next.faults(at(5)).unwrap().len(), 1);

    let first = next.retire_gone::<Puller>(at(5)).unwrap();
    let second = next.retire_gone::<Puller>(at(6)).unwrap();

    assert_eq!(first.iter().map(|record| (record.instance.as_str(), record.state)).collect::<Vec<_>>(), [("gex45", Pull::Abandoned)]);
    assert!(second.is_empty());
    assert!(next.faults(at(7)).unwrap().is_empty());
    let moves: Vec<_> = next.history("pull", "gex45").unwrap().into_iter().map(|logged| (logged.to, logged.cause)).collect();
    assert_eq!(moves, [("pulling".to_owned(), "pulling".to_owned()), ("abandoned".to_owned(), "the holder, pid 10 on laptop, is gone".to_owned())]);
}
