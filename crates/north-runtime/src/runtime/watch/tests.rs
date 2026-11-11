use super::{RESTART_S, Window, capacity_parked, launch_failures, lasting_host_exits, named, risen};
use std::collections::HashSet;
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, State, Store};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Live,
    Parked,
    Stopped,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Live, Self::Parked, Self::Stopped];

    fn name(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Parked => "parked",
            Self::Stopped => "stopped",
        }
    }
}

struct Session;

impl Lifecycle for Session {
    const NAME: &'static str = "fleet-session";
    type State = Phase;

    fn legal(_: Option<Phase>, _: Phase) -> bool {
        true
    }

    fn dwell(_: Phase) -> Option<Duration> {
        None
    }

    fn orphan(_: Phase) -> Option<Phase> {
        None
    }
}

fn failed(reasons: &[&str]) -> Vec<String> {
    reasons.iter().map(|reason| (*reason).to_owned()).collect()
}

#[test]
fn a_kind_rolls_the_runtime_back_only_when_it_ran_faster_since_the_swap_than_the_hour_before() {
    let quiet = Window::default();
    let exits = |count: usize| Window { host_exits: (0..count).map(|n| format!("north-{n}")).collect(), ..Window::default() };
    assert_eq!(risen(&exits(1), &quiet, 300), Vec::<&str>::new(), "one stray exit rolls nothing back");
    assert_eq!(risen(&exits(2), &quiet, 300), ["session-host exits"]);
    assert_eq!(risen(&exits(2), &exits(30), 300), Vec::<&str>::new(), "2 in 5 min is slower than 30 an hour");
    let launching = Window { launches: 6, ..Window::default() };
    let broken = Window { launches: 1, failures: failed(&["session host died at start: exit 101", "session host died at start: exit 101"]), land_refusals: 3, ..Window::default() };
    assert_eq!(risen(&broken, &launching, 600), ["launch failures", "land refusals"]);
}

#[test]
fn a_session_host_restarted_onto_the_new_runtime_is_not_an_exit_and_one_that_stays_down_is() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("ledger.db");
    let mut store = Store::open(&ledger).unwrap();
    let at = |seconds: i64| Timestamp::from_unix_seconds(1_800_000_000 + seconds);
    let mut stops = |session: &str, stopped: i64, started_again: Option<i64>| {
        store.transition::<Session>(session, Phase::Live, "launched", at(stopped - 100)).unwrap().unwrap();
        store.transition::<Session>(session, Phase::Parked, "the holder is gone", at(stopped)).unwrap().unwrap();
        if let Some(again) = started_again {
            store.transition::<Session>(session, Phase::Live, "revived", at(again)).unwrap().unwrap();
        }
    };
    stops("north-1@1", 200, Some(205));
    stops("vn-2@1", 250, None);
    stops("north-3@1", 300, None);
    stops("north-4@1", 400, Some(400 + RESTART_S + 10));
    stops("north-5@1", 590, None);
    let span = (at(0), at(1000));

    let none = HashSet::new();
    assert_eq!(lasting_host_exits(&ledger, span, at(600), &none).len(), 2,"north-3 stayed down and north-4 came back too late; north-1 restarted, vn-2 is a scratch host, north-5 stopped too recently to tell");
    assert_eq!(lasting_host_exits(&ledger, span, at(700), &none).len(), 3, "north-5 is still down once the restart window has passed");
    let lasting = |span| Window { host_exits: lasting_host_exits(&ledger, span, at(700), &none), ..Window::default() };
    assert_eq!(risen(&lasting((at(0), at(260))), &Window::default(), 260), Vec::<&str>::new(), "a restart and a scratch host are no exits");
    assert_eq!(risen(&lasting(span), &Window::default(), 1000), ["session-host exits"]);
}

#[test]
fn launch_failures_on_a_host_with_no_launches_before_the_swap_or_with_a_cause_the_previous_runtime_shares_roll_nothing_back() {
    let failed = Window { failures: failed(&["session host died at start: exit 101", "no login for claude:c", "provider limit: claude:e is at its provider's five-hour limit"]), ..Window::default() };
    assert_eq!(risen(&failed, &Window::default(), 120), Vec::<&str>::new(), "a host that just began launching has no rate to beat");
    let capacity = |load: &str| format!("Capacity leases and unleased heavy load commit {load} of the 8 CPUs the machine can hand out, protected desktop pressure is 0.02% (limit 20%)");
    let before = Window { launches: 6, failures: vec![capacity("10.19"), capacity("10.19"), capacity("10.19")], ..Window::default() };
    let since = Window { failures: vec![capacity("10.79"), capacity("10.79")], ..Window::default() };
    assert_eq!(risen(&since, &before, 50), Vec::<&str>::new(), "the hour before refused for the same cause, so the previous runtime shares it (north#1081)");
}

fn ledger(rows: &[(&str, &str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("threads.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE boundary_causes(session TEXT NOT NULL, source TEXT NOT NULL, cause TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', at TEXT NOT NULL) STRICT").unwrap();
    for (at, cause, reason) in rows {
        db.execute("INSERT INTO boundary_causes VALUES ('worker', 'launch', ?1, ?2, ?3)", [cause, reason, at]).unwrap();
    }
    (home, path)
}

fn iso(seconds: i64) -> String {
    Timestamp::from_unix_seconds(seconds).iso()
}

#[test]
fn a_refused_or_queued_launch_is_not_a_launch_failure_for_the_runtime_watch() {
    let at = "2026-11-11T00:50:00Z";
    let (_home, path) = ledger(&[
        (at, "launch refusal", "n1036-admission would start on gex45, but its prompt names /home/tom/.local/state/north/decisions/admission/, under this host's home, which gex45 cannot read"),
        (at, "launch refusal", "cpx62b skipped: 4 jobs queued in machine-capacity admission (limit 4); it starts there once the host has room"),
        (at, "launch refusal", "provider limit: claude:e is at its provider's five-hour limit"),
        (at, "launch failure", "the claim of north#1109 is only queued: the threads ledger did not take it within 30 s"),
    ]);
    assert_eq!(launch_failures(&path, "2026-11-11T00:00:00Z", "2026-11-11T01:00:00Z"), ["the claim of north#1109 is only queued: the threads ledger did not take it within 30 s"]);
}

/// On 2026-11-11 at 7:58 AM +07 the watch rolled whiterabbit back from ca9d6ae1 to bbd09095 for "launch failures" in the 582 s after its swap, all of them launches placement refused or queued on purpose.
#[test]
fn the_2026_11_11_rollback_of_ca9d6ae1_does_not_happen() {
    let (swapped, judged) = (1_794_357_810, 1_794_358_392);
    let rows: Vec<(&str, &str, &str)> = include_str!("../fixtures/launch-refusals-whiterabbit-2026-11-11.tsv").lines().filter_map(|line| line.split_once('\t')).map(|(at, reason)| (at, "launch refusal", reason)).collect();
    let (_home, path) = ledger(&rows);
    let read = |start: i64, end: i64, launches: usize| Window { launches, failures: launch_failures(&path, &iso(start), &iso(end)), ..Window::default() };
    let (since, prior) = (read(swapped, judged, 0), read(swapped - 3600, swapped, 82));
    assert_eq!(risen(&since, &prior, judged - swapped), Vec::<&str>::new());

    let counted = |start: &str, end: &str, launches: usize| Window { launches, failures: rows.iter().filter(|(at, ..)| *at >= start && *at < end).map(|(.., reason)| (*reason).to_owned()).collect(), ..Window::default() };
    let (since, prior) = (counted(&iso(swapped), &iso(judged), 0), counted(&iso(swapped - 3600), &iso(swapped), 82));
    assert_eq!(risen(&since, &prior, judged - swapped), ["launch failures"], "the replay holds the refusals that rolled ca9d6ae1 back when counted as failures");
    assert!(named(&["launch failures"], &since, &prior).contains("provider limit: claude:e is at its provider's five-hour limit"));
}

/// On 2026-11-11 at about 8:56 AM +07 the watch rolled 7c3b1b1b back for "session-host exits" that were the fleet parking account d's sessions at its limit.
#[test]
fn a_session_parked_by_its_account_limit_is_not_a_session_host_exit() {
    let directory = tempfile::tempdir().unwrap();
    let lifecycle = directory.path().join("ledger.db");
    let mut store = Store::open(&lifecycle).unwrap();
    let at = |seconds: i64| Timestamp::from_unix_seconds(1_800_000_000 + seconds);
    for (session, stopped) in [("n1106-no-diagnosis@1", 100), ("n1022-depbox@1", 110), ("north-3@1", 120)] {
        store.transition::<Session>(session, Phase::Live, "launched", at(stopped - 50)).unwrap().unwrap();
        store.transition::<Session>(session, Phase::Parked, "the holder is gone", at(stopped)).unwrap().unwrap();
    }
    let (_threads, threads) = ledger(&[]);
    let db = rusqlite::Connection::open(&threads).unwrap();
    for session in ["n1106-no-diagnosis", "n1022-depbox"] {
        db.execute("INSERT INTO boundary_causes VALUES (?1, 'fleet', 'capacity', 'parked until account d has usage again', ?2)", [session, &at(105).iso()]).unwrap();
    }
    let parked = capacity_parked(&threads, &at(0).iso(), &at(400).iso());
    assert_eq!(lasting_host_exits(&lifecycle, (at(0), at(400)), at(400), &parked), ["north-3"],"only north-3 stopped on its own");
}

/// On 2026-11-11 from 9:35 AM +07 the watch rolled whiterabbit back four times for "session-host exits" that were sessions the fleet stopped with `north fleet stop` and hosts whose only turn the account's five-hour limit refused.
#[test]
fn a_session_stopped_by_order_or_refused_by_its_account_limit_is_not_a_runtime_exit() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("ledger.db");
    let mut store = Store::open(&ledger).unwrap();
    let at = |seconds: i64| Timestamp::from_unix_seconds(1_800_000_000 + seconds);
    let mut ended = |session: &str, host_ended: i64, then: Option<(i64, &str)>| {
        store.transition::<Session>(session, Phase::Live, "a session host started for it", at(host_ended - 100)).unwrap().unwrap();
        store.transition::<Session>(session, Phase::Parked, "the holder is gone", at(host_ended)).unwrap().unwrap();
        if let Some((later, cause)) = then {
            store.transition::<Session>(session, Phase::Stopped, cause, at(later)).unwrap().unwrap();
        }
    };
    ended("n1022-depbox@1", 100, Some((108, "north fleet stop or forget ended the turn")));
    ended("n1081-reexec@1", 120, Some((123, "stopped after its first turn failed to start")));
    ended("n1063-views@1", 140, Some((150, "retired")));
    ended("north-9@1", 160, None);
    ended("north-8@1", 180, Some((180 + RESTART_S + 30, "retired")));
    let (span, judged, none) = ((at(0), at(500)), at(500), HashSet::new());
    let exits = lasting_host_exits(&ledger, span, judged, &none);
    assert_eq!(exits, ["north-8", "north-9"], "a host that stopped on its own is an exit, even when the fleet retired its session minutes later");

    let ordered = Window { host_exits: lasting_host_exits(&ledger, (at(0), at(155)), at(500), &none), ..Window::default() };
    assert_eq!(risen(&ordered, &Window::default(), 155), Vec::<&str>::new(), "stops by order and a limit-refused host roll nothing back");
    let since = Window { host_exits: exits, ..Window::default() };
    let risen = risen(&since, &Window::default(), 500);
    assert_eq!(risen, ["session-host exits"]);
    assert_eq!(named(&risen, &since, &Window::default()), "session-host exits (north-8, north-9)");
}
