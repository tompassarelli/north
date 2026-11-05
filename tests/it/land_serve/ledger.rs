use std::path::PathBuf;

use north_landing::land::queue;

use super::{Fixture, executable, now_micros};

type Row = (String, String, i64, i64, i64, String, Option<String>, Option<String>, String, Option<String>, Option<String>, String);

fn ledger_rows(fixture: &Fixture, expected: usize) -> Vec<Row> {
    let ledger = fixture.root.join("threads.db");
    for _ in 0..200 {
        if let Ok(db) = rusqlite::Connection::open(&ledger)
            && let Ok(rows) = db
                .prepare("SELECT lane, verdict, batch_size, bisect_depth, attempts, candidate, landed_sha, first_commit_at, queued_at, check_started_at, check_finished_at, coalesce(landed_at, '') FROM landings ORDER BY lane")
                .and_then(|mut statement| statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?)))?.collect::<Result<Vec<Row>, _>>())
            && rows.len() >= expected
        {
            return rows;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the ledger never held {expected} landings rows");
}

fn iso(seconds: f64) -> String {
    north_core::foundation::clock::Timestamp::from_unix_seconds_f64(seconds).iso()
}

#[test]
fn serve_records_the_landed_lane_with_its_first_commit_queue_check_and_landing_times() {
    let mut fixture = Fixture::new();
    let lane = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&lane, None);
    let first_commit = (now_micros() / 1_000_000) as i64 - 1_800;
    let mut queued = queue::read(&ticket).unwrap();
    queued.first_commit_secs = Some(first_commit);
    queue::write(&ticket, &queued).unwrap();
    assert!(fixture.serve().status.success());
    let main = fixture.main();
    let rows = ledger_rows(&fixture, 1);
    assert_eq!(rows.len(), 1);
    let (lane, verdict, batch, depth, attempts, candidate, landed_sha, first, queued, started, finished, landed) = rows[0].clone();
    assert_eq!((lane.as_str(), verdict.as_str(), batch, depth, attempts), ("a", "landed", 1, 0, 1));
    assert_eq!((candidate, landed_sha.as_deref()), (main.clone(), Some(main.as_str())));
    assert_eq!(first.as_deref(), Some(iso(first_commit as f64).as_str()));
    let (queued, started, finished) = (queued, started.unwrap(), finished.unwrap());
    assert!(first.unwrap() < queued && queued <= started && started <= finished && finished <= landed, "first commit, queued, check start, check end and landed are in order: {queued} {started} {finished} {landed}");
}

#[test]
fn serve_records_a_conflict_and_a_failed_check_as_refusals_beside_the_landings() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = [("a", "a.txt"), ("b", "shared.txt"), ("c", "shared.txt"), ("d", "fail.txt")].iter().map(|(name, file)| fixture.lane(name, file, name)).collect();
    for lane in &lanes {
        fixture.enqueue(lane, None);
    }
    executable(&fixture.trace.join("hook"), "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo 'refused https://github.com/o/r/actions/runs/4242' >\"$LAND_TRACE/verdict-$1\"\n");
    assert!(fixture.serve().status.success());
    let rows = ledger_rows(&fixture, 4);
    let verdicts: Vec<(&str, &str)> = rows.iter().map(|row| (row.0.as_str(), row.1.as_str())).collect();
    assert_eq!(verdicts, [("a", "landed"), ("b", "landed"), ("c", "conflict"), ("d", "failed")]);
    let (a, d) = (&rows[0], &rows[3]);
    assert!(a.3 >= 1, "a and b landed after the refused candidate was halved, so their bisect depth is above zero: {a:?}");
    assert!(d.6.is_none() && d.9.is_some() && d.10.is_some(), "the failed lane keeps its check window and has no landed sha: {d:?}");
    let incidents = incident_rows(&fixture, 2);
    assert_eq!(incidents, [("lander-conflict".to_owned(), "lander".to_owned(), "lane-c".to_owned()), ("lander-refused".to_owned(), "lander".to_owned(), "lane-d".to_owned())]);
}

fn incident_rows(fixture: &Fixture, expected: usize) -> Vec<(String, String, String)> {
    let ledger = fixture.root.join("threads.db");
    for _ in 0..200 {
        if let Ok(db) = rusqlite::Connection::open(&ledger)
            && let Ok(rows) = db
                .prepare("SELECT class, noticed_by, subject FROM incidents WHERE started_at IS NOT NULL AND started_at <= detected_at ORDER BY class")
                .and_then(|mut statement| statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?.collect::<Result<Vec<_>, _>>())
            && rows.len() >= expected
        {
            return rows;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the ledger never held {expected} incidents rows");
}
