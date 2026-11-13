use super::super::layout::Layout;
use super::super::live::Swap;
use super::exits::RESTART_S;
use super::{Verdict, Window, capacity_parked, judge, launch_failures, lasting_host_exits, named, risen};
use std::collections::HashSet;
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, Liveness, Owner, State, Store};
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

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::Live).then_some(Phase::Parked)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Delivered;

impl State for Delivered {
    const ALL: &'static [Self] = &[Self];

    fn name(self) -> &'static str {
        "delivered"
    }
}

struct Row;

impl Lifecycle for Row {
    const NAME: &'static str = "row-delivery";
    type State = Delivered;

    fn legal(_: Option<Delivered>, _: Delivered) -> bool {
        true
    }

    fn dwell(_: Delivered) -> Option<Duration> {
        None
    }

    fn orphan(_: Delivered) -> Option<Delivered> {
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

/// On 2026-11-11 at 00:58 UTC the watch rolled whiterabbit back from ca9d6ae1 to bbd09095 for "launch failures" in the 582 s after its swap, all of them launches placement refused or queued on purpose.
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

struct Gone;

impl Liveness for Gone {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        Some(owner.pid != 10)
    }
}

/// Each rolled whiterabbit back for exits someone asked for: on 2026-11-11 at about 01:56 UTC the fleet parking account d's sessions at its limit, from 02:35 UTC sessions `north fleet stop` ended and hosts whose only turn the account's five-hour limit refused, and on 2026-11-13 at 09:33 UTC finished sessions their hosts parked at `--idle-exit`.
#[test]
fn asked_for_exits_do_not_roll_a_runtime_back() {
    let directory = tempfile::tempdir().unwrap();
    let lifecycle = directory.path().join("ledger.db");
    let process = |pid: u32| Owner { host: "laptop".into(), pid, started: 1, booted_ms: 1 };
    let at = |seconds: i64| Timestamp::from_unix_seconds(1_800_000_000 + seconds);
    let mut host = Store::open_as(&lifecycle, process(10), Box::new(Gone)).unwrap();
    let mut reader = Store::open_as(&lifecycle, process(11), Box::new(Gone)).unwrap();
    let mut died = |session: &str, host_ended: i64, then: Option<(i64, &str)>| {
        host.transition::<Session>(session, Phase::Live, "a session host started for it", at(host_ended - 100)).unwrap().unwrap();
        reader.settle::<Session>(session, at(host_ended)).unwrap();
        if let Some((later, cause)) = then {
            reader.transition::<Session>(session, Phase::Stopped, cause, at(later)).unwrap().unwrap();
        }
    };
    died("n1022-depbox@1", 100, Some((108, "north fleet stop or forget ended the turn")));
    died("n1081-reexec@1", 120, Some((123, "stopped after its first turn failed to start")));
    died("n1063-views@1", 140, Some((150, "retired")));
    died("north-9@1", 160, None);
    died("north-8@1", 180, Some((180 + RESTART_S + 30, "retired")));
    died("n1106-no-diagnosis@1", 200, None);
    for (session, parked) in [("f-agent-opus-a@1", 220), ("f-rebase-gap@1", 240)] {
        host.transition::<Session>(session, Phase::Live, "a session host started for it", at(parked - 100)).unwrap().unwrap();
        host.transition::<Session>(session, Phase::Parked, "its host parked it at its idle limit", at(parked)).unwrap().unwrap();
    }
    let (_threads, threads) = ledger(&[]);
    rusqlite::Connection::open(&threads).unwrap().execute("INSERT INTO boundary_causes VALUES ('n1106-no-diagnosis', 'fleet', 'capacity', 'parked until account d has usage again', ?1)", [&at(205).iso()]).unwrap();
    let parked = capacity_parked(&threads, &at(0).iso(), &at(500).iso());
    let exits = |end: i64| lasting_host_exits(&lifecycle, (at(0), at(end)), at(500), &parked);
    assert_eq!(exits(500), ["north-8", "north-9"], "a host that died unasked is an exit, even when the fleet retired its session minutes later");

    let window = |start: i64, end: i64| Window { host_exits: lasting_host_exits(&lifecycle, (at(start), at(end)), at(500), &parked), ..Window::default() };
    assert_eq!(risen(&window(0, 155), &Window::default(), 155), Vec::<&str>::new(), "stops by order and a limit-refused host roll nothing back");
    assert_eq!(risen(&window(195, 300), &Window::default(), 105), Vec::<&str>::new(), "a capacity park and two sessions their hosts parked at idle roll nothing back");
    let since = Window { host_exits: exits(500), ..Window::default() };
    let risen = risen(&since, &Window::default(), 500);
    assert_eq!(risen, ["session-host exits"]);
    assert_eq!(named(&risen, &since, &Window::default()), "session-host exits (north-8, north-9)");
}

/// On 2026-11-10 at about 22:55 UTC the watch rolled 26ae80bb back on whiterabbit for session-host exits from 22:50:52 to 22:54:46 that the runtime did not cause: the canary's three probe sessions, the verify walk's vn-1794351061, the lander-precheck unit after its turn, and a DeepSeek worker that finished its box before the swap. A session that started a turn under the new runtime and died before finishing it, launched after the swap or moved onto it in place, is still an exit. A host the supervisor started again within `RESTART_S` is a restart, and one that stopped more recently than that is not yet known to be either.
#[test]
fn the_watch_counts_only_sessions_that_died_mid_work_under_the_new_runtime() {
    let state = tempfile::tempdir().unwrap();
    let ledger = state.path().join("ledger.db");
    let process = |pid: u32| Owner { host: "whiterabbit".into(), pid, started: 1, booted_ms: 1 };
    let (swapped, judged) = (1_794_351_045, 1_794_351_350);
    let at = Timestamp::from_unix_seconds;
    let mut host = Store::open_as(&ledger, process(10), Box::new(Gone)).unwrap();
    let runs = rusqlite::Connection::open(&ledger).unwrap();
    runs.execute_batch("CREATE TABLE runs(session TEXT NOT NULL, ended_ms INTEGER NOT NULL, exit TEXT NOT NULL)").unwrap();
    let completed = |session: &str, ended: i64| runs.execute("INSERT INTO runs VALUES (?1, ?2, 'completed')", rusqlite::params![session, at(ended).unix_millis()]).unwrap();
    let sessions = [
        ("canary-pi-1484999-1@1", swapped + 2, 1_794_351_052),
        ("canary-claude-1485004-1@1", swapped + 3, 1_794_351_100),
        ("canary-opencode-1485001-1@1", swapped + 4, 1_794_351_130),
        ("vn-1794351061@1", 1_794_351_061, 1_794_351_150),
        ("lander-precheck@1", swapped + 10, 1_794_351_180),
        ("north-1022-d2116-old-state-stores-for-threads-db-graph-db-capacit@1", swapped - 1800, 1_794_351_210),
        ("north-1115-mid-turn@1", swapped + 20, 1_794_351_230),
        ("north-1081-adopted@1", swapped - 1200, 1_794_351_286),
        ("north-1-restarted@1", swapped + 30, 1_794_351_200),
        ("north-4-late@1", swapped + 40, 1_794_351_240),
        ("north-5-recent@1", swapped + 50, 1_794_351_300),
    ];
    for (session, started, _) in sessions {
        host.transition::<Session>(session, Phase::Live, "a session host started for it", at(started)).unwrap().unwrap();
    }
    completed("lander-precheck", 1_794_351_170);
    completed("north-1022-d2116-old-state-stores-for-threads-db-graph-db-capacit", swapped - 600);
    completed("north-1081-adopted", swapped - 900);
    host.transition::<Row>("north-1081-adopted/7", Delivered, "delivered", at(swapped + 60)).unwrap().unwrap();
    let mut reader = Store::open_as(&ledger, process(11), Box::new(Gone)).unwrap();
    for (session, _, ended) in sessions {
        reader.settle::<Session>(session, at(ended)).unwrap();
    }
    let mut supervisor = Store::open_as(&ledger, process(12), Box::new(Gone)).unwrap();
    for (session, again) in [("north-1-restarted@1", 1_794_351_205), ("north-4-late@1", 1_794_351_240 + RESTART_S + 10)] {
        supervisor.transition::<Session>(session, Phase::Live, "a session host started for it", at(again)).unwrap().unwrap();
    }

    let none = HashSet::new();
    let window = |start: i64, end: i64, judged: i64| Window { host_exits: lasting_host_exits(&ledger, (at(start), at(end)), at(judged), &none), ..Window::default() };
    let replay = window(swapped, 1_794_351_220, judged);
    assert_eq!(replay.host_exits, Vec::<String>::new(), "probes, a finished turn, a host idle since before the swap and a host restarted within RESTART_S are no exits");
    assert_eq!(risen(&replay, &window(swapped - 3600, swapped, judged), 175), Vec::<&str>::new());
    let since = window(swapped, judged, judged);
    assert_eq!(since.host_exits, ["north-1081-adopted", "north-1115-mid-turn", "north-4-late"], "a launched turn and a taken row that never finished are exits, and a host restarted too late is one; north-5-recent stopped too recently to tell");
    assert_eq!(window(swapped, judged + 100, judged + 100).host_exits, ["north-1081-adopted", "north-1115-mid-turn", "north-4-late", "north-5-recent"], "north-5-recent is still down once the restart window has passed");
    assert_eq!(risen(&since, &window(swapped - 3600, swapped, judged), judged - swapped), ["session-host exits"]);
}

/// On 2026-11-13 the watch rolled cd052409 back at 09:33 UTC, then 439abac8, which that rollback had made live, at 09:35 UTC, and the window that opened rolled the runtime back again at 09:43 UTC; here the exits of those windows are hosts nobody stopped.
#[test]
fn a_window_a_rollback_opened_is_reported_and_rolls_nothing_back() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let process = |pid: u32| Owner { host: "whiterabbit".into(), pid, started: 1, booted_ms: 1 };
    let at = Timestamp::from_unix_seconds;
    let mut host = Store::open_as(&layout.ledger(), process(10), Box::new(Gone)).unwrap();
    let exits = [("f-agent-opus-a@1", 1_794_562_237), ("f-rebase-gap@1", 1_794_562_313), ("f-queue-wait@1", 1_794_562_429), ("burnb2-fin-op16@1", 1_794_562_465)];
    for (session, ended) in exits {
        host.transition::<Session>(session, Phase::Live, "a session host started for it", at(1_794_560_000)).unwrap().unwrap();
        host.transition::<Row>(&format!("{}/1", session.split_once('@').unwrap().0), Delivered, "delivered", at(ended - 30)).unwrap().unwrap();
    }
    let mut reader = Store::open_as(&layout.ledger(), process(11), Box::new(Gone)).unwrap();
    for (session, ended) in exits {
        reader.settle::<Session>(session, at(ended)).unwrap();
    }
    let (cd05, rolled_back_to) = ("cd052409c8e80a3d4489e48e0f4b0e00d52754b9", "439abac812037a75dbfcb407aa81b4f5f18667a5");
    let swaps = [Swap { runtime: cd05.into(), at: 1_794_561_807, estimated: false, rollback: false }, Swap { runtime: rolled_back_to.into(), at: 1_794_562_394, estimated: false, rollback: true }];
    assert!(matches!(judge(&layout, cd05, &swaps[..1], 1_794_562_394, "whiterabbit"), Verdict::RollBack { seconds: 587, .. }), "a promoted runtime still rolls back on exits nobody asked for");
    let Verdict::Report { why, .. } = judge(&layout, rolled_back_to, &swaps, 1_794_562_542, "whiterabbit") else { panic!("the window the rollback opened rolled back again") };
    assert_eq!(why, "session-host exits (burnb2-fin-op16, f-queue-wait) rose in the 148s since 439abac8 came back by rollback on whiterabbit, over the hour before");
}
