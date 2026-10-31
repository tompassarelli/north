use rusqlite::Connection;

use super::*;
use crate::error::NorthError;

const OLD_EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('claim','release','KICKOFF','ANSWER')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE INDEX events_item ON events(item, seq);";

fn ledger() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    let renamed = OLD_EVENTS.replacen("CREATE TABLE events(", "CREATE TABLE events_widened(", 1).replace("CREATE INDEX events_item ON events", "CREATE INDEX events_item ON events_widened");
    Connection::open(&path).unwrap().execute_batch(&format!("{renamed}\nALTER TABLE events_widened RENAME TO events;")).unwrap();
    (directory, path)
}

fn message(id: &str, to: &str) -> FleetRow {
    FleetRow { id: id.into(), kind: RowKind::Message, to: to.into(), from: "whiterabbit".into(), urgent: false, sent_at: 1_793_000_000_000, text: "hello".into(), sender: None, gate: None, job: None }
}

fn count(path: &std::path::Path, sql: &str) -> i64 {
    Connection::open(path).unwrap().query_row(sql, [], |row| row.get(0)).unwrap()
}

fn session(name: &str, host: &str, status: &str) -> SessionRow {
    SessionRow { name: name.into(), host: host.into(), status: status.into(), created: "2026-10-31T10:00:00Z".into(), updated: "2026-10-31T10:00:00Z".into() }
}

#[test]
fn a_row_put_twice_leaves_one_row() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();

    store.put(&[message("m-1", "sc-1")]).unwrap();
    store.put(&[message("m-1", "sc-1"), message("m-2", "sc-2")]).unwrap();

    assert_eq!(count(&path, "SELECT count(*) FROM events WHERE kind = 'MESSAGE'"), 2);
    assert_eq!(count(&path, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-1'"), 1);
    assert_eq!(count(&path, "SELECT count(*) FROM events WHERE json_extract(data, '$.to_node') = 'sc-1' AND actor = 'whiterabbit'"), 1);
}

#[test]
fn a_ledger_that_predates_the_fleet_kinds_is_widened_with_its_rows_and_indexes_kept() {
    let (_directory, path) = ledger();
    Connection::open(&path).unwrap().execute("INSERT INTO events(ts, kind, data) VALUES ('t', 'KICKOFF', '{}')", []).unwrap();

    Store::open(&path).unwrap().put(&[message("m-1", "sc-1")]).unwrap();
    Store::open(&path).unwrap();

    assert_eq!(count(&path, "SELECT count(*) FROM events"), 2);
    assert_eq!(count(&path, "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name IN ('events_item', 'events_fleet_id')"), 2);
}

#[test]
fn the_id_index_leaves_other_events_with_no_id_alone() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();

    connection.execute("INSERT INTO events(ts, kind, data) VALUES ('t', 'KICKOFF', '{}')", []).unwrap();
    connection.execute("INSERT INTO events(ts, kind, data) VALUES ('t', 'KICKOFF', '{}')", []).unwrap();
    store.put(&[message("m-1", "sc-1")]).unwrap();

    assert_eq!(count(&path, "SELECT count(*) FROM events"), 3);
}

#[test]
fn a_batch_with_one_invalid_row_commits_nothing() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();

    let refused = store.put(&[message("m-1", "sc-1"), message("not an id", "sc-1")]);

    assert!(matches!(refused, Err(NorthError::Usage(_))));
    assert_eq!(count(&path, "SELECT count(*) FROM events"), 0);
}

#[test]
fn rows_parse_from_one_object_or_an_array_and_take_the_letter_field_to() {
    let one = br#"{"id":"m-1","kind":"MESSAGE","to":"sc-1","from":"nexus","sent_at":5,"text":"hi"}"#;
    let two = br#"[{"id":"m-1","kind":"JOB-EXIT","to_node":"sc-1","from":"nexus","urgent":true,"sent_at":5,"job":{"key":"j","exit":0}},{"id":"m-2","kind":"GATE","to":"sc-1","from":"nexus","sent_at":6}]"#;

    let rows = FleetRow::parse(one).unwrap();
    assert_eq!((rows.len(), rows[0].to.as_str(), rows[0].urgent), (1, "sc-1", false));
    let rows = FleetRow::parse(two).unwrap();
    assert_eq!((rows.len(), rows[0].kind, rows[0].job.as_ref().and_then(|job| job.exit)), (2, RowKind::JobExit, Some(0)));
    assert!(serde_json::to_string(&rows[0]).unwrap().contains(r#""to_node":"sc-1""#));
    assert!(FleetRow::parse(br#"{"id":"m-1","kind":"NOTE","to":"a","from":"b","sent_at":1}"#).is_err());
    assert!(FleetRow::parse(br#"{"id":"","kind":"MESSAGE","to":"a","from":"b","sent_at":1}"#).is_err());
}

#[test]
fn a_launch_claims_a_new_name_and_is_refused_while_a_session_of_that_name_is_not_exited() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();

    store.register(&session("sc-1", "nexus", "working")).unwrap();
    let Err(NorthError::Refused(why)) = store.register(&session("sc-1", "gex45", "starting")) else { panic!("a live name was claimed twice") };

    assert_eq!(why, "the session name sc-1 is held fleet-wide by a working session on nexus");
    assert_eq!(store.sessions().unwrap(), [session("sc-1", "nexus", "working")]);
}

#[test]
fn a_launch_takes_over_the_name_of_an_exited_session() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.register(&session("sc-1", "nexus", "exited")).unwrap();

    store.register(&session("sc-1", "gex45", "starting")).unwrap();

    assert_eq!(store.sessions().unwrap(), [session("sc-1", "gex45", "starting")]);
}

#[test]
fn a_host_publishes_its_registry_without_taking_a_live_name_from_another_host() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.register(&session("sc-1", "nexus", "working")).unwrap();
    store.register(&session("sc-2", "nexus", "exited")).unwrap();

    let held = store.publish(&[session("sc-1", "gex45", "idle"), session("sc-2", "gex45", "idle"), session("sc-3", "gex45", "parked")]).unwrap();
    let held_again = store.publish(&[session("sc-3", "gex45", "working")]).unwrap();

    assert_eq!(held, ["sc-1"]);
    assert!(held_again.is_empty());
    assert_eq!(store.sessions().unwrap(), [session("sc-1", "nexus", "working"), session("sc-2", "gex45", "idle"), session("sc-3", "gex45", "working")]);
}

#[test]
fn a_ledger_without_events_is_refused_and_a_missing_ledger_is_not_created() {
    let directory = tempfile::tempdir().unwrap();
    let bare = directory.path().join("bare.db");
    Connection::open(&bare).unwrap().execute_batch("CREATE TABLE t(v)").unwrap();

    assert!(Store::open(&bare).is_err());
    assert!(Store::open(&directory.path().join("missing.db")).is_err());
    assert!(!directory.path().join("missing.db").exists());
}

#[test]
fn an_events_table_declared_unquoted_is_widened_too() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(OLD_EVENTS).unwrap();
    Store::open(&path).unwrap().put(&[message("m-1", "sc-1")]).unwrap();
    assert_eq!(count(&path, "SELECT count(*) FROM events WHERE kind = 'MESSAGE'"), 1);
}

#[test]
fn a_spooled_row_is_one_file_that_lists_until_withdrawn_and_a_bad_id_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let spool = Spool::at(root.path());

    assert!(spool.pending().unwrap().is_empty());
    spool.put(&message("m-2", "sc-1")).unwrap();
    spool.put(&message("m-1", "sc-1")).unwrap();
    spool.put(&message("m-1", "sc-1")).unwrap();

    assert_eq!(spool.pending().unwrap(), [message("m-1", "sc-1"), message("m-2", "sc-1")]);
    assert!(root.path().join("spool/m-1.json").is_file());
    assert!(spool.put(&message("../m-3", "sc-1")).is_err());
    spool.withdraw("m-1").unwrap();
    spool.withdraw("m-1").unwrap();
    assert_eq!(spool.pending().unwrap(), [message("m-2", "sc-1")]);
}

#[test]
fn the_cached_session_table_round_trips_and_a_host_without_one_knows_no_sessions() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(cached_sessions(root.path()).unwrap(), None);
    cache_sessions(root.path(), &[session("sc-1", "nexus", "working")]).unwrap();
    cache_sessions(root.path(), &[session("sc-2", "gex45", "idle"), session("sc-1", "nexus", "working")]).unwrap();

    assert_eq!(cached_sessions(root.path()).unwrap(), Some(vec![session("sc-2", "gex45", "idle"), session("sc-1", "nexus", "working")]));
}
