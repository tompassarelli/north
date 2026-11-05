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
    FleetRow { id: id.into(), kind: RowKind::Message, to: to.into(), from: "whiterabbit".into(), urgent: false, sent_at: 1_793_000_000_000, text: "hello".into(), sender: crate::fleet::sender::Sender::Tom, gate: None, answer: None, job: None, origin: None }
}

fn count(path: &std::path::Path, sql: &str) -> i64 {
    Connection::open(path).unwrap().query_row(sql, [], |row| row.get(0)).unwrap()
}

fn session(name: &str, host: &str, status: &str) -> SessionRow {
    SessionRow { name: name.into(), host: host.into(), status: status.into(), created: "2026-10-31T10:00:00Z".into(), updated: "2026-10-31T10:00:00Z".into(), ..SessionRow::default() }
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
fn a_launch_claims_a_new_name_and_is_refused_while_a_session_of_that_name_is_not_exited() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();

    store.register(&session("sc-1", "nexus", "working"), Claim::Fresh).unwrap();
    let Err(NorthError::Refused(why)) = store.register(&session("sc-1", "gex45", "starting"), Claim::Fresh) else { panic!("a live name was claimed twice") };

    assert_eq!(why, "the session name sc-1 is held fleet-wide by a working session on nexus");
    assert_eq!(store.sessions().unwrap(), [session("sc-1", "nexus", "working")]);
}

#[test]
fn withdrawing_many_ids_removes_exactly_those_rows_ignores_unknown_ids_and_refuses_a_path_like_id_whole() {
    let root = tempfile::tempdir().unwrap();
    let spool = Spool::at(root.path());
    ["m-1", "m-2", "m-3"].iter().for_each(|id| spool.put(&message(id, "sc-1")).unwrap());

    spool.withdraw_all(&["m-1".into(), "m-9".into(), "m-3".into()]).unwrap();
    assert_eq!(spool.pending().unwrap(), [message("m-2", "sc-1")]);

    assert!(spool.withdraw_all(&["m-2".into(), "../m-2".into()]).is_err());
    assert!(spool.withdraw_all(&["a/b".into()]).is_err());
    assert!(spool.withdraw_all(&[String::new()]).is_err());
    assert_eq!(spool.pending().unwrap(), [message("m-2", "sc-1")]);
}

#[test]
fn a_stored_row_that_no_longer_parses_is_skipped_and_the_rest_are_delivered() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.put(&[message("m-bad", "sc-1"), message("m-good", "sc-1")]).unwrap();
    Connection::open(&path).unwrap().execute("UPDATE events SET data = json_set(data, '$.sender', 'two words') WHERE json_extract(data, '$.id') = 'm-bad'", []).unwrap();

    let exchanged = store.exchange(&Exchange { addressees: vec![Addressee { name: "sc-1".into(), ..Addressee::default() }], ..Exchange::default() }).unwrap();

    assert_eq!(exchanged.rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["m-good"]);
}

#[test]
fn a_spooled_row_that_does_not_parse_is_set_aside_and_the_rest_flush() {
    let root = tempfile::tempdir().unwrap();
    let spool = Spool::at(root.path());
    spool.put(&message("m-1", "sc-1")).unwrap();
    std::fs::write(root.path().join("spool/m-0.json"), r#"{"id":"m-0","kind":"MESSAGE","to_node":"sc-1","sender":"two words"}"#).unwrap();

    assert_eq!(spool.pending().unwrap(), [message("m-1", "sc-1")]);
    assert!(root.path().join("spool/rejected/m-0.json").is_file());
    assert!(!root.path().join("spool/m-0.json").exists());
}

