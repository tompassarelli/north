use std::path::PathBuf;

use rusqlite::Connection;

use super::run;
use crate::foundation::clock::Timestamp;
use crate::threads::fake::Recorded;
use crate::threads::verb::Context;

const NIGHT: &str = include_str!("fixtures/2026-11-04-night.jsonl");
const NOW: f64 = 1_793_800_000.0;

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
    fn call(&self, words: &[&str], input: &str, now: f64) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: None, outside: &self.outside, now: Timestamp::from_unix_seconds_f64(now), replica: false };
        let argv: Vec<String> = std::iter::once("incident").chain(words.iter().copied()).map(str::to_owned).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &context, &mut input.as_bytes(), &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn ok(&self, words: &[&str]) -> String {
        let (code, out, err) = self.call(words, "", NOW);
        assert_eq!((code, err.as_str()), (0, ""), "{words:?}");
        out
    }

    fn record(&self, key: &str, class: &str, noticed_by: &str, started: f64, detected: f64) -> String {
        self.ok(&["record", "--key", key, "--class", class, "--noticed-by", noticed_by, "--repo", "north", "--started-at", &started.to_string(), "--detected-at", &detected.to_string()])
    }
}

#[test]
fn the_backfilled_night_reads_nine_classes_none_noticed_by_a_detector_first() {
    let ledger = ledger();
    let (code, out, err) = ledger.call(&["import"], NIGHT, NOW);
    assert_eq!((code, err.as_str(), out.as_str()), (0, "", "incident import: 9 of 9 rows recorded, 3 guards set\n"));
    let night = ["report", "--since", "1793811600", "--until", "1793898000"];
    let report = ledger.ok(&night);
    assert!(report.contains("incidents 2026-11-04T17:00:00Z to 2026-11-05T17:00:00Z: detector-first 0 of 9 (0%)"), "{report}");
    assert!(report.contains(": 9 by class: clippy-failure-at-queue-time 1, conflict-send-back-livelock 1, half-failed-fleet-launch 1, lander-hang 1,"), "{report}");
    assert!(report.contains("median to detect 442 min (n=2), to fix 20 min (n=3)"), "{report}");
    assert!(report.contains("no class recurred after its guard landed"), "{report}");
    let (_, again, _) = ledger.call(&["import"], NIGHT, NOW);
    assert_eq!(again, "incident import: 0 of 9 rows recorded, 3 guards set\n");
}

#[test]
fn an_incident_after_its_class_guard_landed_is_a_recurrence_and_one_before_is_fixed_by_it() {
    let ledger = ledger();
    let hour = 3600.0;
    ledger.record("a", "lander-hang", "proxy", NOW - 5.0 * hour, NOW - 4.0 * hour);
    ledger.ok(&["guard", "--class", "lander-hang", "--guard", "63d9e7783", "--landed-at", &(NOW - 2.0 * hour).to_string(), "--issue", "tompassarelli/north#360"]);
    assert_eq!(ledger.record("b", "lander-hang", "lander", NOW - hour - 600.0, NOW - hour), "incident b: recorded\n");
    assert_eq!(ledger.record("b", "lander-hang", "lander", NOW - hour - 600.0, NOW - hour), "incident b: already recorded\n");
    ledger.record("c", "lander-conflict", "lander", NOW - 1200.0, NOW - 600.0);
    let db = Connection::open(&ledger.path).unwrap();
    let rows: Vec<(String, Option<String>, String, i64)> =
        db.prepare("SELECT key, fixed_at, guard, recurred_after_guard FROM incidents ORDER BY key").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap().collect::<Result<_, _>>().unwrap();
    let fixed = Timestamp::from_unix_seconds_f64(NOW - 2.0 * hour).iso();
    assert_eq!(
        rows,
        [("a".into(), Some(fixed), "63d9e7783".into(), 0), ("b".into(), None, "63d9e7783".into(), 1), ("c".into(), None, String::new(), 0)]
    );
    let report = ledger.ok(&["report"]);
    assert!(report.contains("incidents 7d: 3 by class: lander-conflict 1, lander-hang 2"), "{report}");
    assert!(report.contains("detector-first 2 of 3 (67%); median to detect 10 min (n=3), to fix 120 min (n=1)"), "{report}");
    assert!(report.contains("recurred after their guard landed: lander-hang 1 (guard 63d9e7783)"), "{report}");
    let recurred = super::report::week(&ledger.path, NOW).unwrap().recurred;
    assert_eq!((recurred.len(), recurred[0].issue.as_str(), recurred[0].key.as_str()), (1, "tompassarelli/north#360", "b"));
}

#[test]
fn a_ledger_without_incidents_reads_none_and_a_bad_record_is_refused() {
    let ledger = ledger();
    assert!(super::report::week(&ledger.path, NOW).is_none());
    assert!(ledger.ok(&["report"]).contains("incidents 7d: 0 by class: none"));
    assert_eq!(ledger.call(&["record", "--class", "x", "--noticed-by", "proxy"], "", NOW).0, 2);
    assert_eq!(ledger.call(&["record", "--key", "k", "--class", "x", "--noticed-by", "proxy", "--detected-at", "soon"], "", NOW).0, 2);
    assert_eq!(ledger.call(&["guard", "--class", "x", "--guard", "abc"], "", NOW).0, 2);
    assert_eq!(ledger.call(&["import"], "{\"key\":\"k\"}\n", NOW).0, 2);
}
