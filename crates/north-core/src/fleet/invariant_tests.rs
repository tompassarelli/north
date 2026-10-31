use super::*;

const ENDED_MS: u64 = 1_793_534_340_000;

fn now() -> Timestamp {
    Timestamp::parse("2026-11-01T12:00:00Z").unwrap()
}

fn at(offset_secs: i64) -> String {
    Timestamp::from_unix_seconds(now().unix_seconds() + offset_secs).iso()
}

fn wait(kind: WaitKind, key: &str, deadline: String) -> WaitRow {
    WaitRow { session: "box-1".into(), kind, key: key.into(), deadline, created: at(-600) }
}

fn turn(reply: &str) -> Option<LastTurn> {
    Some(LastTurn { reply: reply.into(), id: 1, ended_ms: ENDED_MS, exit: TurnExit::Completed })
}

fn parked<'a>(waits: Vec<&'a WaitRow>) -> Worker<'a> {
    Worker { running: false, gated: false, turn: turn("Still building on nexus."), waits, registered_here: false, pull_failed: false }
}

fn reason_of(verdict: &Verdict) -> &str {
    match verdict {
        Verdict::Wake { reason, .. } => reason,
        other => panic!("not a wake: {other:?}"),
    }
}

#[test]
fn a_worker_parked_with_no_registered_wait_is_woken_after_two_ticks_of_grace() {
    let worker = parked(Vec::new());
    let (first, mark) = judge(&worker, Mark::default(), now());
    let (second, mark) = judge(&worker, mark, now());
    assert_eq!((first, second), (Verdict::Grace, Verdict::Grace), "a JOB-START still in a spool gets two ticks");
    let (third, mark) = judge(&worker, mark, now());
    assert_eq!(reason_of(&third), "woken: parked with no registered wait");
    assert_eq!(mark.wakes, 1);
    assert_eq!(judge(&worker, mark, now()).0, Verdict::Holds, "one wake per turn, never twice for the same stop");
}

#[test]
fn a_job_wait_whose_lease_lapsed_with_no_exit_wakes_naming_the_job_and_settles_the_wait() {
    let lapsed = wait(WaitKind::Job, "nexus-build", at(-1));
    let (verdict, _) = judge(&parked(vec![&lapsed]), Mark::default(), now());
    assert_eq!(verdict, Verdict::Wake { reason: "woken: your job nexus-build's lease lapsed with no exit".into(), settled: vec![lapsed.key()] });
    let leased = wait(WaitKind::Job, "nexus-build", at(60));
    assert_eq!(judge(&parked(vec![&leased]), Mark::default(), now()).0, Verdict::Holds, "a live lease holds the park");
}

#[test]
fn a_wait_past_its_deadline_wakes_naming_the_wait() {
    let late = wait(WaitKind::Farm, "4242", at(0));
    let (verdict, _) = judge(&parked(vec![&late]), Mark::default(), now());
    assert_eq!(reason_of(&verdict), "woken: wait farm:4242 passed its deadline");
    let due = wait(WaitKind::Branch, "claude/land-x", at(3600));
    assert_eq!(judge(&parked(vec![&due]), Mark::default(), now()).0, Verdict::Holds);
}

#[test]
fn a_machine_whose_spool_pull_failed_this_tick_gets_no_lapse_check() {
    let lapsed = wait(WaitKind::Job, "gex45-capture", at(-120));
    let worker = Worker { pull_failed: true, ..parked(vec![&lapsed]) };
    assert_eq!(judge(&worker, Mark::default(), now()).0, Verdict::Holds, "a beat may still sit in the unpulled spool");
    let worker = Worker { pull_failed: false, ..worker };
    assert_eq!(reason_of(&judge(&worker, Mark::default(), now()).0), "woken: your job gex45-capture's lease lapsed with no exit");
}

#[test]
fn a_worker_with_an_open_gate_a_final_report_or_a_question_is_left_alone() {
    let stale = Mark { turn: ENDED_MS, unwaited: GRACE_TICKS, ..Mark::default() };
    assert_eq!(judge(&Worker { gated: true, ..parked(Vec::new()) }, stale, now()).0, Verdict::Holds);
    for reply in ["Done: landed abc123.", "Not done: the farm failed.", "Split: two boxes.", "Stop: capacity.", "Blocked: on Tom.", "Which target do you want?"] {
        let (verdict, mark) = judge(&Worker { turn: turn(reply), ..parked(Vec::new()) }, stale, now());
        assert_eq!((verdict, mark), (Verdict::Holds, Mark::default()), "{reply}");
    }
    assert_eq!(judge(&Worker { running: true, ..parked(Vec::new()) }, stale, now()).0, Verdict::Holds);
    let interrupted = Worker { turn: Some(LastTurn { exit: TurnExit::Interrupted, ..turn("Still building.").unwrap() }), ..parked(Vec::new()) };
    assert_eq!(judge(&interrupted, stale, now()).0, Verdict::Holds, "only a completed turn is judged");
    assert_eq!(judge(&Worker { registered_here: true, ..parked(Vec::new()) }, stale, now()).0, Verdict::Holds, "a pid wait registered on this host holds the park");
}

#[test]
fn the_fourth_wake_tells_the_parent_instead() {
    let worker = parked(Vec::new());
    let mut mark = Mark::default();
    for wake in 1..=WAKES {
        let turn = ENDED_MS + u64::from(wake);
        let stopped = Worker { turn: Some(LastTurn { id: turn, ..worker.turn.clone().unwrap() }), ..worker.clone() };
        mark = Mark { turn, unwaited: GRACE_TICKS, acted: false, ..mark };
        let (verdict, next) = judge(&stopped, mark, now());
        assert!(matches!(verdict, Verdict::Wake { .. }), "wake {wake}: {verdict:?}");
        mark = next;
    }
    let stopped = Worker { turn: Some(LastTurn { id: ENDED_MS + 9, ..worker.turn.clone().unwrap() }), ..worker.clone() };
    let (verdict, _) = judge(&stopped, Mark { turn: ENDED_MS + 9, unwaited: GRACE_TICKS, acted: false, ..mark }, now());
    assert_eq!(verdict, Verdict::Stalled { settled: Vec::new() });
    assert!(stalled("box-1", "Still building.").contains("woken 3 times without a final report"));
}

#[test]
fn a_valid_wait_or_a_final_report_starts_the_wake_count_over() {
    let leased = wait(WaitKind::Job, "build", at(60));
    let (_, mark) = judge(&parked(vec![&leased]), Mark { turn: ENDED_MS, wakes: 2, ..Mark::default() }, now());
    assert_eq!(mark.wakes, 0);
}

#[test]
fn a_stop_older_than_the_judged_window_is_left_alone() {
    let window = u64::try_from(JUDGED_FOR.as_millis()).unwrap();
    let old = Worker { turn: Some(LastTurn { ended_ms: ENDED_MS + 60_000 - window, ..turn("Still building.").unwrap() }), ..parked(Vec::new()) };
    assert_eq!(judge(&old, Mark { unwaited: GRACE_TICKS, ..Mark::default() }, now()).0, Verdict::Holds, "a stop from before the invariant ran is not revived");
    let late = wait(WaitKind::Branch, "claude/land-x", at(-1));
    let day_old = Worker { turn: Some(LastTurn { ended_ms: ENDED_MS - 24 * 3600 * 1000, ..turn("Waiting on the landing.").unwrap() }), ..parked(vec![&late]) };
    assert_eq!(reason_of(&judge(&day_old, Mark::default(), now()).0), "woken: wait branch:claude/land-x passed its deadline", "the longest default deadline still falls inside the window");
}

#[test]
fn a_wake_keeps_its_row_id_for_the_same_turn_however_the_streams_write_time_moves() {
    let mark = Mark { turn: 7, wakes: 1, acted: true, ..Mark::default() };

    let first = wake_row("nexus", "box-1", mark, ENDED_MS, "woken: parked with no registered wait");
    let later = wake_row("nexus", "box-1", mark, ENDED_MS + 90_000, "woken: parked with no registered wait");
    let next_wake = wake_row("nexus", "box-1", Mark { wakes: 2, ..mark }, ENDED_MS, "woken: parked with no registered wait");

    assert_eq!(first.id, later.id);
    assert_ne!(first.id, next_wake.id);
    assert_ne!(first.id, wake_row("nexus", "box-1", Mark { turn: 8, ..mark }, ENDED_MS, "woken: parked with no registered wait").id);
}

#[test]
fn work_running_below_the_cli_holds_a_stopped_turn_for_two_hours_and_then_the_wake_resumes() {
    let held = turn("Still building.").unwrap();
    let after = |millis: i64| Timestamp::from_unix_millis(i64::try_from(ENDED_MS).unwrap() + millis);

    assert!(held_by_cli_work(&held, after(2 * 3_600_000 - 1)));
    assert!(!held_by_cli_work(&held, after(2 * 3_600_000)));
}
