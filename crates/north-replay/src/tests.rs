use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use north_base::platform::decision_log;
use north_strategy_types::admission::{self, Class, Profile, Reason};
use north_strategy_types::github::Quota;
use north_strategy_types::land::{self, BatchCase, BatchDecision, JudgeCase, Judgement, Verdict};
use north_strategy_types::placement::{self, Outcome, Target, Window};
use north_strategy_types::{Envelope, Kind, Snapshot};
use serde::Serialize;

use super::*;

const NOW: f64 = 1_793_920_000.0;

fn append(root: &Path, kind: Kind, suffix: &str, at: f64, line: &impl Serialize) {
    let path = decision_log::hour_file(root, kind, at, suffix);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    writeln!(std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap(), "{}", serde_json::to_string(line).unwrap()).unwrap();
}

fn envelope<C, D>(kind: Kind, id: &str, at: f64, follows: Option<&str>, case: C, decision: D) -> Envelope<C, D> {
    Envelope { v: 1, kind, id: id.to_owned(), at, host: "h".into(), strategy: "live:test".into(), follows: follows.map(str::to_owned), case, decision, decide_us: 1, dropped: 0 }
}

fn reading(land_check_waiting: bool) -> admission::Snapshot {
    admission::Snapshot {
        profile: Profile::Attended,
        cores: 24,
        slice_cpus: None,
        memory_total_mib: 96_000.0,
        memory_available_mib: 70_000.0,
        cpu_some_avg10_basis_points: 0,
        protected_cpu_some_avg10_basis_points: 0,
        cpu_pressure_measured: false,
        leased_memory_mib: 0.0,
        leased_batch_cpus: 0.0,
        unleased_heavy_cores: 0.0,
        leased_native_cpus: 0.0,
        peer_batch_runs: 0,
        peer_exclusive_runs: 0,
        unbounded_runs: 0,
        gpu_busy_percent: 0,
        gpu_clients: 0,
        host_warcraft_clients: None,
        bar1_free_mib: None,
        bar1_timed_out: false,
        vram_free_mib: None,
        host_bar1_margin_mib: None,
        gpu_runs: 0,
        native_waiting: 0,
        land_check_waiting,
        leases: Vec::new(),
        queue: Vec::new(),
        exclusive_leases: Vec::new(),
        queued_pair_leases: Vec::new(),
        gaming: false,
        game_reserve_agent_cpus: None,
        run_queue_load: None,
        disk_free_gib: None,
    }
}

fn request(slot: bool) -> admission::Request {
    admission::Request {
        now: NOW * 1000.0,
        class: Class::Heavy,
        gaming_class: None,
        requested_cpus: 2.0,
        requested_memory_mib: 2048.0,
        new_clients: 0,
        new_desktops: 0,
        queued_ahead: 0,
        exclusive_waiting: 0,
        queued_ahead_tickets: Vec::new(),
        queued_ahead_new_clients: Vec::new(),
        before_drain: false,
        alone: false,
        owner: None,
        pair: None,
        beside_native: false,
        quiet: false,
        ticket: Some("t".into()),
        requester_lease: None,
        need: None,
        within_exclusive: false,
        holds_offload_slot: slot,
    }
}

fn decision(reason: Reason) -> admission::Decision {
    admission::Decision { reason, offload: None, bar1_need_mib: None, disk_free_gib: None, pair: None, quiet_owners: Vec::new(), reclaim: false }
}

fn log_admission(root: &Path, count: usize, slot: bool, logged: impl Fn(usize) -> Reason) {
    let hash = if slot { "waiting" } else { "free" };
    let held = Snapshot { v: 1, hash: hash.into(), at: NOW - 5000.0, host: "h".into(), snapshot: reading(slot) };
    append(root, Kind::Admission, "snapshots.jsonl", NOW - 5000.0, &held);
    for index in 0..count {
        let case = admission::Case { request: request(slot), snapshot: hash.into(), settle_seconds: 0.0 };
        append(root, Kind::Admission, "jsonl", NOW - 4000.0 + index as f64, &envelope(Kind::Admission, &format!("a{index}"), NOW - 4000.0 + index as f64, None, case, decision(logged(index))));
    }
}

fn bounds() -> Bounds {
    Bounds { five_hour_floor_points: 5.0, week_floor_points: 0.0, github_floor_calls: 1000.0, landing_p90_seconds: 60.0, shift_ratio: 2.0, shift_min_cases: 20, budget_seconds: 5.0 }
}

fn deciders<'a>(
    admission: &'a AdmissionDecider,
    judge: &'a (dyn Fn(&land::JudgeCase) -> Judgement + Sync),
    placement: &'a (dyn Fn(&placement::Case) -> Option<placement::Target> + Sync),
) -> Deciders<'a> {
    Deciders { admission, judge, placement }
}

fn quiet_admission(_: &str, _: &admission::Request) -> Option<(Reason, bool)> {
    Some((Reason::Run, false))
}

fn same_judgement(case: &JudgeCase) -> Judgement {
    if matches!(case.verdict, Verdict::Green) { Judgement::Land } else { Judgement::Hold(String::new()) }
}

fn unplaced(_: &placement::Case) -> Option<placement::Target> {
    None
}

fn exit(root: &Path, deciders: &Deciders) -> (u8, Replayed) {
    let code = execute(root, NOW, deciders, &bounds(), &Options { since: 86_400.0, invariants: true, budget: false });
    (code, replay(root, NOW - 86_400.0, NOW, deciders, &bounds()))
}

#[test]
fn a_change_that_shifts_counts_or_places_a_launch_under_the_quota_floor_is_refused_and_an_unchanged_one_is_not() {
    let root = tempfile::tempdir().unwrap();
    log_admission(root.path(), 300, false, |index| if index % 12 == 0 { Reason::DeferQueued } else { Reason::Run });
    let defers_everything = |_: &str, _: &admission::Request| Some((Reason::DeferQueued, false));
    let (code, replayed) = exit(root.path(), &deciders(&defers_everything, &same_judgement, &unplaced));
    assert_eq!(code, 1);
    let refusals = replayed.refusals(&bounds(), false);
    assert_eq!(refusals.len(), 2, "{refusals:?}");
    assert!(refusals.iter().any(|refusal| refusal.starts_with("admission DEFER_QUEUED: logged 25, replayed 300")), "{refusals:?}");

    let unchanged = tempfile::tempdir().unwrap();
    log_admission(unchanged.path(), 300, false, |_| Reason::Run);
    let (code, replayed) = exit(unchanged.path(), &deciders(&quiet_admission, &same_judgement, &unplaced));
    assert_eq!((code, &replayed.counts[0].logged), (0, &replayed.counts[0].replayed));

    let placed = tempfile::tempdir().unwrap();
    for (index, used) in [20.0, 96.0].into_iter().enumerate() {
        let at = NOW - 200.0 + index as f64;
        append(placed.path(), Kind::Placement, "jsonl", at, &envelope(Kind::Placement, &format!("p{index}"), at, None, placed_case(used), placed_on_c()));
    }
    let places_anyway = |_: &placement::Case| Some(Target::Account("claude:c".into()));
    let (code, replayed) = exit(placed.path(), &deciders(&quiet_admission, &same_judgement, &places_anyway));
    let quota = replayed.invariants.iter().find(|invariant| invariant.name == "quota-floor").unwrap();
    assert_eq!((code, quota.checked, quota.passed), (1, 4, 3));
    assert!(quota.breaches[0].contains("4 points of its five-hour window left"), "{:?}", quota.breaches);
    assert_github_floor_refuses_a_launch_placed_under_1000_calls_and_skips_unread_cases();
}

fn placed_case(used_5h: f64) -> placement::Case {
    let window = |name: &str, used: f64| Window { window: Some(name.into()), used_pct: Some(used), resets_at: None, reset_h: None, burn_pct_h: None };
    let controls = BTreeMap::from([("claude:c".to_owned(), vec![window("5h", used_5h), window("week", 10.0)])]);
    placement::Case {
        at: NOW - 100.0,
        launch: placement::Launch { name: "box".into(), account: "c".into(), provider: "claude".into(), ..Default::default() },
        brief: String::new(),
        config: placement::Config::default(),
        credit: placement::Credit::default(),
        usage: placement::Usage { controls, ..Default::default() },
        pace: BTreeMap::new(),
        roster: BTreeMap::new(),
        machines: Vec::new(),
        here: placement::Here::default(),
        github: None,
    }
}

fn with_github(mut case: placement::Case, remaining: u64) -> placement::Case {
    case.github = Some(Quota { resource: "core".into(), remaining, resets_at: NOW + 600.0 });
    case
}

fn placed_on_c() -> placement::Decision {
    placement::Decision { target: Target::Account("claude:c".into()), machine: None, outcome: Outcome::Placed { account: Some("c".into()), note: None, machine: None, need: None }, lines: Vec::new() }
}

fn log_landings(root: &Path, walls: &[f64]) {
    for (index, wall) in walls.iter().enumerate() {
        let started = NOW - 3000.0 + index as f64 * 100.0;
        let id = format!("b{index}");
        let batch = BatchCase { base: "m".into(), tickets: Vec::new(), picks: Vec::new(), limit: None, red_only: false, pause: None, strikes: BTreeMap::new(), spares: 0, rebuilds: 0, bisect_depth: 0, github: None };
        let included = vec![format!("lane{index}")];
        append(root, Kind::LandBatch, "jsonl", started, &envelope(Kind::LandBatch, &id, started, None, batch, BatchDecision { included: included.clone(), halve: None }));
        let case = JudgeCase { verdict: Verdict::Green, included, base_moved: false, streak: 0, searched: false };
        append(root, Kind::LandJudge, "jsonl", started + wall, &envelope(Kind::LandJudge, &format!("j{index}"), started + wall, Some(&id), case, Judgement::Land));
    }
}

#[test]
fn the_landing_tail_breaches_on_slow_rounds_and_on_landings_the_replay_withholds_and_times_each_ticket_once() {
    let slow = tempfile::tempdir().unwrap();
    log_landings(slow.path(), &[5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 90.0, 95.0, 100.0]);
    let (code, replayed) = exit(slow.path(), &deciders(&quiet_admission, &same_judgement, &unplaced));
    let tail = replayed.invariants.iter().find(|invariant| invariant.name == "landing-p90").unwrap();
    assert_eq!((code, tail.checked, tail.passed), (1, 10, 7));
    assert!(tail.breaches[0].contains("p90 is 95.0 s over 60 s") && tail.breaches[0].contains("2026-11-05"), "{:?}", tail.breaches);

    let held = tempfile::tempdir().unwrap();
    log_landings(held.path(), &[5.0; 10]);
    let holds = |_: &JudgeCase| Judgement::Hold("new rule".into());
    let (code, replayed) = exit(held.path(), &deciders(&quiet_admission, &holds, &unplaced));
    let tail = replayed.invariants.iter().find(|invariant| invariant.name == "landing-p90").unwrap();
    assert_eq!((code, tail.passed), (1, 0));
    assert!(tail.breaches[0].contains("never landed under replay: 10"), "{:?}", tail.breaches);

    let early = tempfile::tempdir().unwrap();
    for (index, logged) in [Judgement::Halve(1), Judgement::Land].into_iter().enumerate() {
        let started = NOW - 3000.0 + index as f64 * 100.0;
        let id = format!("b{index}");
        let batch = BatchCase { base: "m".into(), tickets: Vec::new(), picks: Vec::new(), limit: None, red_only: false, pause: None, strikes: BTreeMap::new(), spares: 0, rebuilds: 0, bisect_depth: 0, github: None };
        append(early.path(), Kind::LandBatch, "jsonl", started, &envelope(Kind::LandBatch, &id, started, None, batch, BatchDecision { included: vec!["lane".into()], halve: None }));
        let case = JudgeCase { verdict: Verdict::Green, included: vec!["lane".into()], base_moved: false, streak: 0, searched: false };
        append(early.path(), Kind::LandJudge, "jsonl", started + 5.0, &envelope(Kind::LandJudge, &format!("j{index}"), started + 5.0, Some(&id), case, logged));
    }
    let (_, replayed) = exit(early.path(), &deciders(&quiet_admission, &same_judgement, &unplaced));
    let tail = replayed.invariants.iter().find(|invariant| invariant.name == "landing-p90").unwrap();
    assert_eq!(tail.checked, 1);
}

#[test]
fn reapplying_the_parent_of_8d4c5d10a_to_slot_claims_exits_non_zero_naming_slot_time() {
    let root = tempfile::tempdir().unwrap();
    log_admission(root.path(), 30, true, |_| Reason::Run);
    let linked = Deciders::linked();
    let (code, replayed) = exit(root.path(), &linked);
    assert_eq!((code, replayed.refusals(&bounds(), true).len()), (0, 0));
    let slot = replayed.invariants.iter().find(|invariant| invariant.name == "slot-time").unwrap();
    assert_eq!((slot.checked, slot.passed), (30, 30));

    let parent = |snapshot: &str, request: &admission::Request| (linked.admission)(snapshot, &admission::Request { holds_offload_slot: false, ..request.clone() });
    let (code, replayed) = exit(root.path(), &deciders(&parent, &same_judgement, &unplaced));
    assert_eq!(code, 1);
    assert!(replayed.refusals(&bounds(), true).iter().any(|refusal| refusal.starts_with("slot-time:")), "{:?}", replayed.refusals(&bounds(), true));
}

fn assert_github_floor_refuses_a_launch_placed_under_1000_calls_and_skips_unread_cases() {
    let root = tempfile::tempdir().unwrap();
    for (index, remaining) in [Some(4000), Some(900), None, Some(29)].into_iter().enumerate() {
        let at = NOW - 200.0 + index as f64;
        let case = remaining.map_or_else(|| placed_case(20.0), |remaining| with_github(placed_case(20.0), remaining));
        let case = if index == 3 { placement::Case { github: case.github.map(|quota| Quota { resource: "search".into(), ..quota }), ..case } } else { case };
        append(root.path(), Kind::Placement, "jsonl", at, &envelope(Kind::Placement, &format!("p{index}"), at, None, case, placed_on_c()));
    }
    let places = |_: &placement::Case| Some(Target::Account("claude:c".into()));
    let (code, replayed) = exit(root.path(), &deciders(&quiet_admission, &same_judgement, &places));
    let floor = replayed.invariants.iter().find(|invariant| invariant.name == "github-floor").unwrap();
    assert_eq!((code, floor.checked, floor.passed), (1, 2, 1));
    assert!(floor.breaches[0].contains("launch box is placed on claude:c with 900 GitHub core calls left"), "{:?}", floor.breaches);
}
