use std::path::PathBuf;

use rusqlite::Connection;
use serde_json::json;

use super::{run, tempo};
use crate::foundation::clock::Timestamp;
use crate::threads::fake::Recorded;
use crate::threads::verb::Context;

const NOW: f64 = 1_793_800_000.0;
const MINUTE: f64 = 60.0;

struct Ledger {
    _directory: tempfile::TempDir,
    path: PathBuf,
    outside: Recorded,
}

fn ledger() -> Ledger {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE unrelated(v TEXT) STRICT;").unwrap();
    Ledger { _directory: directory, path, outside: Recorded::default() }
}

impl Ledger {
    fn call(&self, argv: &[String]) -> (i32, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: None, outside: &self.outside, now: Timestamp::from_unix_seconds_f64(NOW), replica: false };
        let words: Vec<String> = std::iter::once("land-train".to_owned()).chain(argv.iter().cloned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&words, &context, &mut std::io::empty(), &mut out, &mut err);
        (code, String::from_utf8(err).unwrap())
    }

    fn ok(&self, argv: &[String]) {
        assert_eq!(self.call(argv), (0, String::new()), "{argv:?}");
    }

    fn land(&self, lane: &str, queued_ago: f64, landed_ago: Option<f64>, candidate: &str, (batch, depth): (u32, u32), verdict: &str) {
        let mut argv: Vec<String> = ["record", "--repo", "north", "--lane", lane, "--branch", lane, "--tip", &format!("tip-{lane}-{queued_ago}"), "--candidate", candidate, "--tree", &format!("tree-{candidate}")]
            .iter()
            .map(|word| (*word).to_owned())
            .collect();
        argv.extend(["--queued-at".into(), (NOW - queued_ago * MINUTE).to_string(), "--batch-size".into(), batch.to_string(), "--bisect-depth".into(), depth.to_string(), "--attempts".into(), "1".into(), "--verdict".into(), verdict.into()]);
        if let Some(landed) = landed_ago {
            argv.extend(["--landed-at".into(), (NOW - landed * MINUTE).to_string(), "--landed-sha".into(), candidate.into()]);
        }
        self.ok(&argv);
    }

    fn check(&self, candidate: &str, started_ago: f64, outcome: &str) {
        let words = ["check", "--repo", "north", "--candidate", candidate, "--started-at", &(NOW - started_ago * MINUTE).to_string(), "--finished-at", &(NOW - (started_ago - 5.0) * MINUTE).to_string(), "--outcome", outcome];
        self.ok(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>());
    }

    fn count(&self, table: &str) -> i64 {
        Connection::open(&self.path).unwrap().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
    }
}

#[test]
fn a_lane_recorded_twice_leaves_one_row_with_the_later_verdict() {
    let ledger = ledger();
    ledger.land("a", 10.0, None, "c1", (2, 1), "failed");
    assert_eq!(ledger.count("landings"), 1);
    let again: Vec<String> = ["record", "--repo", "north", "--lane", "a", "--branch", "a", "--tip", "tip-a-10", "--queued-at", &(NOW - 10.0 * MINUTE).to_string(), "--verdict", "landed", "--landed-at", &NOW.to_string(), "--batch-size", "1", "--attempts", "2"]
        .iter()
        .map(|word| (*word).to_owned())
        .collect();
    ledger.ok(&again);
    let db = Connection::open(&ledger.path).unwrap();
    let (verdict, attempts): (String, i64) = db.query_row("SELECT verdict, attempts FROM landings", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!((verdict.as_str(), attempts, ledger.count("landings")), ("landed", 2, 1));
}

#[test]
fn a_record_with_an_unknown_verdict_or_missing_time_is_refused() {
    let ledger = ledger();
    let refused = |words: &[&str]| ledger.call(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()).0;
    assert_eq!(refused(&["record", "--repo", "north", "--lane", "a", "--tip", "t", "--queued-at", "1", "--verdict", "maybe"]), 2);
    assert_eq!(refused(&["record", "--repo", "north", "--lane", "a", "--tip", "t", "--verdict", "landed"]), 2);
    assert_eq!(refused(&["check", "--repo", "north", "--candidate", "c", "--started-at", "1", "--finished-at", "2", "--outcome", "meh"]), 2);
    assert_eq!(refused(&["nonsense"]), 2);
}

#[test]
fn the_tempo_counts_landings_waits_batches_discards_relands_bisects_and_red_minutes() {
    let ledger = ledger();
    ledger.land("a", 30.0, Some(20.0), "c1", (3, 0), "landed");
    ledger.land("b", 25.0, Some(20.0), "c1", (3, 0), "landed");
    ledger.land("c", 24.0, Some(20.0), "c1", (3, 1), "landed");
    ledger.land("d", 12.0, Some(10.0), "c2", (1, 0), "landed");
    ledger.land("e", 50.0, None, "c0", (1, 2), "failed");
    ledger.land("e", 9.0, Some(2.0), "c3", (1, 0), "landed");
    ledger.land("old", 3000.0, Some(2990.0), "c9", (1, 5), "landed");
    for (candidate, started, outcome) in [("c0", 55.0, "fail"), ("c1", 40.0, "moved"), ("c1b", 34.0, "pass"), ("c2", 16.0, "pass"), ("c3", 14.0, "pass")] {
        ledger.check(candidate, started, outcome);
    }
    let day_start = (NOW as i64).div_euclid(86_400) as f64 * 86_400.0;
    ledger.ok(&["red", "--repo", "north", "--sha", "bad", "--red-at", &(day_start + 600.0).to_string(), "--green-at", &(day_start + 1800.0).to_string()].map(str::to_owned));

    let tempo = tempo::tempo(&ledger.path, "north", NOW).unwrap();
    assert_eq!(tempo["landings"], 5);
    assert_eq!(tempo["checks_thrown_away"], 1);
    assert_eq!(tempo["check_runs"], 5);
    assert_eq!(tempo["thrown_away_share"], 0.2);
    assert_eq!(tempo["mean_batch_size"], json!((3.0 + 1.0 + 1.0) / 3.0));
    assert_eq!((&tempo["re_lands"], &tempo["re_landed_lanes"]), (&json!(1), &json!(1)));
    assert_eq!(tempo["deepest_bisect"], 2);
    assert_eq!(tempo["red_main_minutes_today"], 20.0);
    assert_eq!(tempo["queue_to_landed_p50_minutes"], 5.0);
    assert_eq!(tempo["queue_to_landed_p90_minutes"], 10.0);
    assert!((tempo["landings_per_hour"].as_f64().unwrap() - 5.0 / 24.0).abs() < 1e-9);
    let line = tempo::line("north", &tempo);
    for part in ["5 landings", "p50 5.0m p90 10.0m", "mean batch 1.7", "re-lands 1 across 1 lanes", "thrown away (main moved) 1/5 = 20%", "deepest bisect 2", "red main today 20m"] {
        assert!(line.contains(part), "{part} missing from {line}");
    }
}

#[test]
fn a_ledger_without_the_tables_or_the_repository_has_no_tempo() {
    let ledger = ledger();
    assert!(tempo::tempo(&ledger.path, "north", NOW).is_none());
    ledger.land("a", 5.0, Some(1.0), "c1", (1, 0), "landed");
    assert!(tempo::tempo(&ledger.path, "wisp", NOW).is_none());
    assert!(tempo::tempo(&ledger.path.with_file_name("missing.db"), "north", NOW).is_none());
}
