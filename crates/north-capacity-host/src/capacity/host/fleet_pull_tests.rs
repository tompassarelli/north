use rusqlite::Connection;

use super::*;
use crate::capacity::host::database::{SCHEMA, add_run_columns};

fn fixture_database() -> Connection {
    let db = Connection::open_in_memory().expect("open");
    db.execute_batch(SCHEMA).expect("schema");
    add_run_columns(&db).expect("run columns");
    db
}

fn remote_rows(rows: &[(&str, &str)]) -> String {
    let entries: Vec<Value> = rows
        .iter()
        .map(|(source, at)| serde_json::json!({"source_id": source, "at": at, "family": "cargo", "repo": "north", "package": null, "class": "heavy", "signature_exact": 1, "wallSeconds": 12.5, "unknownColumn": 1}))
        .collect();
    Value::Array(entries).to_string()
}

#[test]
fn a_pull_adds_only_rows_newer_than_the_last_pulled_one_and_a_second_pull_adds_none() {
    let mut db = fixture_database();
    let remote = remote_rows(&[("a", "2026-11-04T01:00:00.000Z"), ("b", "2026-11-04T02:00:00.000Z")]);
    let asked = std::cell::RefCell::new(Vec::new());
    let first = pull_host(&mut db, "gex45", &|since| {
        asked.borrow_mut().push(since.to_owned());
        Ok(remote.clone())
    });
    assert_eq!(first, Ok(2));
    let second = pull_host(&mut db, "gex45", &|since| {
        asked.borrow_mut().push(since.to_owned());
        Ok(remote.clone())
    });
    assert_eq!(second, Ok(0));
    assert_eq!(*asked.borrow(), vec![String::new(), "2026-11-04T02:00:00.000Z".to_owned()]);
    let newer = remote_rows(&[("b", "2026-11-04T02:00:00.000Z"), ("c", "2026-11-04T03:00:00.000Z")]);
    assert_eq!(pull_host(&mut db, "gex45", &|_| Ok(newer.clone())), Ok(1));
}

#[test]
fn each_pulled_row_carries_its_host_and_two_hosts_may_share_a_source_id() {
    let mut db = fixture_database();
    let remote = remote_rows(&[("jsonl:/state/usage.jsonl:0", "2026-11-04T01:00:00.000Z")]);
    assert_eq!(pull_host(&mut db, "gex45", &|_| Ok(remote.clone())), Ok(1));
    assert_eq!(pull_host(&mut db, "nexus", &|_| Ok(remote.clone())), Ok(1));
    let hosts: Vec<(String, i64)> = db
        .prepare("SELECT host, count(*) FROM capacity_runs GROUP BY host ORDER BY host")
        .expect("query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("rows")
        .flatten()
        .collect();
    assert_eq!(hosts, vec![("gex45".to_owned(), 1), ("nexus".to_owned(), 1)]);
    let wall: f64 = db.query_row("SELECT wallSeconds FROM capacity_runs WHERE host = 'nexus'", [], |row| row.get(0)).expect("wall");
    assert_eq!(wall, 12.5);
}

#[test]
fn an_unreachable_host_adds_nothing_and_reports_why() {
    let mut db = fixture_database();
    assert_eq!(pull_host(&mut db, "gex45", &|_| Err("ssh gex45 exited with 255".to_owned())), Err("ssh gex45 exited with 255".to_owned()));
    assert_eq!(pull_host(&mut db, "gex45", &|_| Ok(String::new())), Ok(0));
}

#[test]
fn rows_from_before_the_host_column_belong_to_this_machine() {
    let db = fixture_database();
    db.execute("INSERT INTO capacity_runs (source_id, at, family, class, signature_exact) VALUES ('x', '2026-11-04T00:00:00Z', 'cargo', 'heavy', 1)", []).expect("legacy row");
    claim_local_rows(&db, "whiterabbit").expect("claimed");
    let host: String = db.query_row("SELECT host FROM capacity_runs WHERE source_id = 'x'", [], |row| row.get(0)).expect("host");
    assert_eq!(host, "whiterabbit");
}

#[test]
fn only_hosts_reached_over_ssh_are_pulled_and_this_machine_is_not() {
    let policy = "[hosts.whiterabbit]\nresident_cap = 8\n[hosts.gex45]\nssh = \"tom@gex45\"\n[hosts.nexus]\ndeploy_ssh = \"nexus\"\n[hosts.other]\nresident_cap = 1\n";
    assert_eq!(ssh_hosts(policy, "whiterabbit"), vec![("gex45".to_owned(), "tom@gex45".to_owned()), ("nexus".to_owned(), "nexus".to_owned())]);
    assert_eq!(ssh_hosts("[hosts.gex45]\nssh = \"tom@gex45\"\n", "gex45"), Vec::<(String, String)>::new());
}

#[test]
fn sizing_a_key_on_this_machine_is_identical_with_and_without_another_hosts_rows() {
    use crate::capacity::host::database::capacity_history_of;
    use crate::capacity::host::estimate::{CapacityQuery, estimate_capacity, task_signature};

    let mut db = fixture_database();
    for index in 0..5 {
        let host = if index % 2 == 0 { "'whiterabbit'" } else { "NULL" };
        db.execute(
            &format!(
                "INSERT INTO capacity_runs (source_id, at, family, repo, class, signature_exact, host, meanCores, p90Cores, peakMemoryMiB, wallSeconds, exitCode) \
                 VALUES ('jsonl:/state/usage.jsonl:{index}', '2026-11-03T0{index}:00:00.000Z', 'cargo test', 'north', 'heavy', 1, {host}, 4, 6, 3000, 60, 0)"
            ),
            [],
        )
        .expect("local row");
    }
    let signature = task_signature(&["cargo".to_owned(), "test".to_owned()], None, "heavy", true);
    let signature = crate::capacity::host::estimate::TaskSignature { repo: Some("north".to_owned()), ..signature };
    let query = CapacityQuery { owner: None, shape: None };
    let sized = |db: &Connection| estimate_capacity(&signature, &capacity_history_of(db, "whiterabbit").expect("history"), 16.0, &query);
    let alone = sized(&db);
    assert_eq!(alone["sampleCount"], 5);

    let remote: Vec<Value> = (0..15)
        .map(|index| {
            serde_json::json!({
                "source_id": format!("jsonl:/state/usage.jsonl:{index}"), "at": format!("2026-11-04T{:02}:00:00.000Z", index), "family": "cargo test", "repo": "north",
                "class": "heavy", "signature_exact": 1, "meanCores": 18, "p90Cores": 20, "peakMemoryMiB": 40000, "wallSeconds": 900, "exitCode": 0
            })
        })
        .collect();
    let remote = Value::Array(remote).to_string();
    assert_eq!(pull_host(&mut db, "gex45", &|_| Ok(remote.clone())), Ok(15));
    let total: i64 = db.query_row("SELECT count(*) FROM capacity_runs", [], |row| row.get(0)).expect("count");
    assert_eq!(total, 20);
    assert_eq!(sized(&db), alone);
}
