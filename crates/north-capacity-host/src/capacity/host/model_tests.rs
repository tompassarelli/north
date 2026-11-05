use rusqlite::Connection;
use serde_json::{Value, json};

use super::super::database::{SCHEMA, insert_capacity_run, migrate};
use super::super::estimate::task_signature;
use super::super::policy::memory_margin;
use super::super::run::learn;
use super::super::sizing::{model_memory, own_host};
use super::{CENSOR_RAISE, DRIFT_MEMORY_WIDEN, MODEL_MINIMUM_RUNS, Observation, SIZING_RUNS, Subject, WINDOW, cache_state, censored, censored_percentile, level_of, predict, rows, update};

pub(in super::super) fn database() -> Connection {
    let mut db = Connection::open_in_memory().expect("open");
    db.execute_batch(SCHEMA).expect("schema");
    migrate(&mut db).expect("migrate");
    db
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn subject(owner: &str, command: &str, host: &str, cache: &str) -> Subject {
    Subject::of(owner, &words(command), host.to_owned(), cache)
}

fn observe(db: &Connection, subject: &Subject, wall: f64, anon: f64, censored_at_mib: Option<f64>) {
    update(db, &Observation { subject, wall_seconds: wall, cpu_seconds: wall * 4.0, anon_peak_mib: anon, censored_at_mib, failed: false }).expect("update");
}

fn fail(db: &Connection, subject: &Subject, wall: f64) {
    update(db, &Observation { subject, wall_seconds: wall, cpu_seconds: wall * 4.0, anon_peak_mib: 9999.0, censored_at_mib: None, failed: true }).expect("update");
}

fn lease_row(exit: i64) -> Value {
    json!({
        "at": "2026-11-04T00:00:00.000Z", "owner": "capacity-model", "shape": "cargo test", "command": ["cargo", "test"], "class": "heavy", "exitCode": exit,
        "wallSeconds": 120.0, "cpuSeconds": 480.0, "peakMemoryMiB": 2048.0, "reservedMemoryMiB": 2048.0, "queuedSeconds": 7.5,
        "key": "capacity cargo test", "host": "gex45", "cache": "warm", "anonPeakMiB": 1700.0, "memoryStallUsec": 3_900_000.0, "censored": true,
        "predictedWallSeconds": 100.0, "predictedWallP90Seconds": 150.0, "predictedMemoryMiB": 1600.0, "predictionBasis": "owner-shape",
    })
}

#[test]
fn a_finished_lease_records_host_cache_anon_peak_stall_censoring_queue_wait_and_prediction_in_one_transaction_and_a_failure_teaches_only_its_duration() {
    let mut db = database();
    let signature = task_signature(&words("cargo test"), None, "heavy", true);
    for (lease, exit) in [("a", 0), ("b", 101)] {
        let transaction = db.transaction().expect("transaction");
        insert_capacity_run(&transaction, &format!("lease:{lease}"), &lease_row(exit), &signature, None).expect("insert");
        learn(&transaction, &lease_row(exit)).expect("learn");
        transaction.commit().expect("commit");
    }
    let stored: (String, String, String, f64, f64, f64, i64, f64, f64, String) = db
        .query_row(
            "SELECT key, host, cache, queuedSeconds, anonPeakMiB, memoryStallUsec, censored, predictedWallSeconds, predictedWallP90Seconds, predictionBasis FROM capacity_runs WHERE source_id = 'lease:a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
        )
        .expect("row");
    assert_eq!(stored, ("capacity cargo test".to_owned(), "gex45".to_owned(), "warm".to_owned(), 7.5, 1700.0, 3_900_000.0, 1, 100.0, 150.0, "owner-shape".to_owned()));
    let model = rows(&db, None).expect("model");
    let levels: Vec<&str> = model.iter().map(|row| level_of(&row.key, &row.cache)).collect();
    assert_eq!(levels, ["command-host", "command-host-cache", "owner-shape", "shape-host"]);
    assert!(model.iter().all(|row| row.host == "gex45" && row.runs == 2 && row.memory.p90.is_nan() && (row.cpu.p50, row.cpu.p90) == (480.0, 480.0) && row.floor_mib == Some(2048.0 * CENSOR_RAISE)));
}

#[test]
fn the_model_keeps_only_its_newest_window_and_is_the_same_for_the_same_rows() {
    let build = || {
        let db = database();
        let subject = subject("north", "cargo build", "laptop", "cold");
        for index in 0..(WINDOW + 8) {
            observe(&db, &subject, index as f64, 1000.0 + index as f64, None);
        }
        rows(&db, None).expect("rows")
    };
    let first = build();
    assert_eq!(first, build());
    let exact = first.iter().find(|row| level_of(&row.key, &row.cache) == "command-host-cache").expect("exact row");
    assert_eq!(exact.runs, (WINDOW + 8) as i64);
    assert_eq!((exact.wall.p50, exact.wall.p90), (17.0, 25.0));
    assert_eq!(exact.memory.p90, 1025.0);
}

#[test]
fn a_new_command_backs_off_through_command_host_then_owner_shape_on_its_own_host_and_is_otherwise_unknown() {
    let db = database();
    let basis = |subject: &Subject| predict(&db, subject).expect("predict").map(|prediction| prediction.basis);
    let known = subject("north", "cargo test -p north-core", "gex45", "cold");
    assert_eq!(basis(&known), None);
    for _ in 0..MODEL_MINIMUM_RUNS {
        observe(&db, &known, 60.0, 3000.0, None);
    }
    assert_eq!(basis(&known), Some("command-host-cache"));
    assert_eq!(basis(&subject("north", "cargo test -p north-core", "gex45", "warm")), Some("command-host"));
    let same_shape = subject("north", "cargo test -p north-base", "gex45", "warm");
    assert_eq!(same_shape.key, known.key);
    assert_eq!(basis(&same_shape), Some("owner-shape"));
    assert_eq!(basis(&subject("north", "cargo test -p north-core", "laptop", "cold")), None);
    assert_eq!(basis(&subject("wisp", "cargo test -p wisp", "gex45", "cold")), Some("shape-host"));
    assert_eq!(basis(&subject("wisp", "bun test", "gex45", "cold")), None);
}

#[test]
fn a_failed_or_timed_out_run_teaches_its_duration_as_a_lower_bound_and_not_its_memory() {
    let db = database();
    let known = subject("north", "cargo test", "gex45", "warm");
    for wall in [10.0, 20.0, 30.0] {
        observe(&db, &known, wall, 1000.0, None);
    }
    let row = || predict(&db, &known).expect("predict").expect("model").row;
    assert_eq!((row().wall.p50, row().wall.p90), (20.0, 30.0));
    fail(&db, &known, 5.0);
    fail(&db, &known, 600.0);
    let taught = row();
    assert_eq!((taught.runs, taught.wall.p50, taught.wall.p90), (5, 20.0, 600.0));
    assert_eq!((taught.memory.p90, taught.cpu.p90), (1000.0, 120.0));
    let timed_out = subject("north", "cargo bench", "gex45", "warm");
    for _ in 0..MODEL_MINIMUM_RUNS {
        fail(&db, &timed_out, 900.0);
    }
    let bound = predict(&db, &timed_out).expect("predict").expect("a key of only timeouts still predicts");
    assert_eq!((bound.row.wall.p50, bound.row.wall.p90), (900.0, 900.0));
    assert!(bound.row.memory.p90.is_nan() && !bound.sizes_memory());
}

#[test]
fn a_censored_percentile_without_bounds_is_the_plain_percentile_and_a_bound_only_shrinks_who_is_at_risk() {
    let plain: Vec<(f64, bool)> = (1..=10).map(|value| (f64::from(value), false)).collect();
    assert_eq!((censored_percentile(&plain, 0.5), censored_percentile(&plain, 0.9), censored_percentile(&plain, 1.0)), (5.0, 9.0, 10.0));
    let early_bound = [(1.0, true), (10.0, false), (20.0, false)];
    assert_eq!((censored_percentile(&early_bound, 0.5), censored_percentile(&early_bound, 0.9)), (10.0, 20.0));
    assert!(censored_percentile(&[], 0.5).is_nan());
}

#[test]
fn a_key_flagged_drift_widens_its_next_memory_margin_and_duration_percentiles_with_no_edit() {
    let db = database();
    let known = subject("north", "cargo test", "gex45", "warm");
    for (wall, anon) in [(10.0, 1000.0), (20.0, 1100.0), (30.0, 1200.0), (40.0, 1300.0), (50.0, 1400.0)] {
        observe(&db, &known, wall, anon, None);
    }
    let prediction = || predict(&db, &known).expect("predict").expect("model");
    let steady = prediction();
    assert!(!steady.drift);
    assert_eq!((steady.row.wall.p50, steady.row.wall.p90, steady.memory_mib(1.0)), (30.0, 50.0, 1400.0));
    let signature = task_signature(&words("cargo test"), None, "heavy", true);
    for index in 0..MODEL_MINIMUM_RUNS {
        let entry = json!({
            "at": format!("2026-11-04T00:0{index}:00.000Z"), "key": known.key, "host": "gex45", "exitCode": 0, "wallSeconds": 30.0,
            "predictedWallSeconds": 30.0, "predictedWallP90Seconds": 50.0, "anonPeakMiB": 2600.0, "predictedMemoryMiB": 1400.0, "predictionBasis": "owner-shape",
        });
        insert_capacity_run(&db, &format!("lease:missed-{index}"), &entry, &signature, None).expect("insert");
    }
    let widened = prediction();
    assert!(widened.drift);
    assert_eq!((widened.row.wall.p50, widened.row.wall.p90), (50.0, 50.0));
    assert!((widened.memory_mib(1.0) - 1400.0 * DRIFT_MEMORY_WIDEN).abs() < 1e-9);
    let sized = model_memory("heavy", Some(&widened), 16_384.0).expect("sized");
    assert!((sized - 1400.0 * memory_margin() * DRIFT_MEMORY_WIDEN).abs() < 1e-9);
    let other_host = subject("north", "cargo test", "laptop", "warm");
    for _ in 0..SIZING_RUNS {
        observe(&db, &other_host, 30.0, 1400.0, None);
    }
    assert!(!predict(&db, &other_host).expect("predict").expect("model").drift);
}

#[test]
fn admission_sizes_memory_from_p90_anon_once_a_key_has_five_runs_and_a_censored_run_only_raises_its_floor() {
    let db = database();
    let known = subject("north", "cargo test", "laptop", "warm");
    for anon in [1000.0, 1100.0, 1200.0, 1300.0] {
        observe(&db, &known, 30.0, anon, None);
    }
    let prediction = |subject: &Subject| predict(&db, subject).expect("predict");
    assert_eq!(model_memory("heavy", prediction(&known).as_ref(), 16_384.0), None);
    observe(&db, &known, 30.0, 1400.0, None);
    assert_eq!(prediction(&known).expect("model").row.runs, SIZING_RUNS);
    let sized = model_memory("heavy", prediction(&known).as_ref(), 16_384.0).expect("sized");
    assert!((sized - 1400.0 * memory_margin()).abs() < 1e-9);
    assert_eq!(model_memory("tom", prediction(&known).as_ref(), 1024.0), Some(1024.0));
    observe(&db, &known, 30.0, 1450.0, Some(2048.0));
    assert_eq!(model_memory("heavy", prediction(&known).as_ref(), 16_384.0), Some(2048.0 * CENSOR_RAISE));
    let stranger = subject("wisp", "bun test", "laptop", "warm");
    assert_eq!(prediction(&stranger), None);
    assert_eq!(model_memory("heavy", None, 16_384.0), None);
    assert_eq!(prediction(&known).expect("model").row.memory.p90, 1400.0);
}

#[test]
fn a_run_is_censored_only_when_it_stalled_over_a_second_pressed_against_its_memory_high() {
    assert!(censored(Some(2_100_000.0), 2040.0, Some(2048.0)));
    assert!(!censored(Some(2_100_000.0), 900.0, Some(2048.0)));
    assert!(!censored(Some(400_000.0), 2048.0, Some(2048.0)));
    assert!(!censored(None, 2048.0, Some(2048.0)));
    assert!(!censored(Some(2_100_000.0), 2048.0, None));
}

#[test]
fn a_lane_is_warm_only_when_its_target_holds_fingerprints() {
    let lane = tempfile::tempdir().expect("lane");
    std::fs::write(lane.path().join("Cargo.lock"), "").expect("lock");
    let nested = lane.path().join("crates/app");
    std::fs::create_dir_all(&nested).expect("crate");
    assert_eq!(cache_state(&nested, None), "cold");
    std::fs::create_dir_all(lane.path().join("target/debug/.fingerprint")).expect("empty fingerprints");
    assert_eq!(cache_state(&nested, None), "cold");
    std::fs::create_dir_all(lane.path().join("target/debug/.fingerprint/app-0123456789abcdef")).expect("fingerprint");
    assert_eq!(cache_state(&nested, None), "warm");
    assert_eq!(cache_state(&nested, Some("elsewhere")), "cold");
}

#[test]
fn sizing_history_holds_only_this_hosts_rows_and_rows_from_before_hosts_were_recorded() {
    let db = database();
    let signature = task_signature(&words("cargo test"), None, "heavy", true);
    let local = crate::platform::host::name();
    for (source, host) in [("here", Some(local.as_str())), ("there", Some("another-host")), ("legacy", None)] {
        let entry = json!({ "at": "2026-11-04T00:00:00.000Z", "host": host, "p90Cores": 2.0, "peakMemoryMiB": 100.0 });
        insert_capacity_run(&db, source, &entry, &signature, None).expect("insert");
    }
    let history = super::super::database::capacity_history(&db).expect("history");
    let hosts: Vec<Option<String>> = own_host(history, &local).into_iter().map(|run| run.host).collect();
    assert_eq!(hosts, [Some(local), None]);
}
