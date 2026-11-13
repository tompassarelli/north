use rusqlite::Connection;

use super::*;
use crate::error::NorthError;
use crate::sender::Sender;

const OLD_EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('claim','release','KICKOFF','ANSWER')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE INDEX events_item ON events(item, seq);";

/// The fleet registry before `WAIT` and `SPEND` rows existed: the events table's kind check and the row-id index are
/// the parts its writable open rewrites in place.
const OLD_STATE: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('MESSAGE','GATE','GATE-ANSWER','JOB-START','JOB-BEAT','JOB-EXIT','CONTROL')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE INDEX events_item ON events(item, seq);
CREATE TABLE fleet_sessions(name TEXT PRIMARY KEY, host TEXT NOT NULL, status TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL, provider TEXT NOT NULL DEFAULT '', account TEXT NOT NULL DEFAULT '', cwd TEXT NOT NULL DEFAULT '', parent TEXT, placement TEXT, machine TEXT, tasks TEXT NOT NULL DEFAULT '[]', conversation TEXT) STRICT;";

#[test]
fn an_old_state_registry_opens_writable_and_read_only_and_keeps_its_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(OLD_STATE).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO fleet_sessions(name, host, status, created, updated, conversation) VALUES ('north-0', 'nexus', 'working', '2026-11-10T00:00:00Z', '2026-11-10T00:00:00Z', 'conversation-1');
             INSERT INTO events(seq, ts, kind, actor, tier, data) VALUES (1, '2026-11-10T00:00:00Z', 'MESSAGE', 'whiterabbit', '', '{\"id\":\"m-1\",\"kind\":\"MESSAGE\",\"to_node\":\"north-0\",\"sender\":\"tom\",\"text\":\"hello\"}');",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    let opened: Vec<String> = store.sessions().unwrap().into_iter().map(|row| row.name).collect();
    assert_eq!(opened, ["north-0"], "the writable open keeps the session row the older build wrote");
    assert_eq!(count(&path, "SELECT count(*) FROM sqlite_schema WHERE name = 'events_fleet_id'"), 1, "the writable open adds the row-id index the older build lacked");
    drop(store);

    let delivered = delivered_by(&path, &crate::sender::Sender::Tom).unwrap();
    assert!(delivered.contains(&("conversation-1".to_owned(), "hello".to_owned())), "the read-only open reads the message row the older build wrote");
}

fn ledger() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    let renamed = OLD_EVENTS.replacen("CREATE TABLE events(", "CREATE TABLE events_widened(", 1).replace("CREATE INDEX events_item ON events", "CREATE INDEX events_item ON events_widened");
    Connection::open(&path).unwrap().execute_batch(&format!("{renamed}\nALTER TABLE events_widened RENAME TO events;")).unwrap();
    (directory, path)
}

fn message(id: &str, to: &str) -> FleetRow {
    FleetRow { id: id.into(), kind: RowKind::Message, to: to.into(), from: "whiterabbit".into(), urgent: false, sent_at: 1_793_000_000_000, text: "hello".into(), sender: crate::sender::Sender::Tom, gate: None, answer: None, job: None, origin: None, spooled_by: None }
}

fn count(path: &std::path::Path, sql: &str) -> i64 {
    Connection::open(path).unwrap().query_row(sql, [], |row| row.get(0)).unwrap()
}

fn session(name: &str, host: &str, status: &str) -> SessionRow {
    SessionRow { name: name.into(), host: host.into(), status: status.into(), created: "2026-10-31T10:00:00Z".into(), updated: Timestamp::now().iso(), ..SessionRow::default() }
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

/// On 2026-11-11 the proxy's exchange with the ledger took 8 to 25 s on nexus because each of the proxy's 5,437 delivered ids was matched by re-reading the addressee's whole JSON for every row, so the tick dropped the answer and no line from a build host reached it.
#[test]
fn an_addressee_with_thousands_of_delivered_ids_is_answered_in_a_few_thousand_steps() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let rows: Vec<FleetRow> = (0..4000).map(|number| message(&format!("m-{number:05}"), "proxy")).collect();
    store.put(&rows).unwrap();
    let taken: Vec<String> = rows.iter().take(3990).map(|row| row.id.clone()).chain((0..3000).map(|number| format!("other-{number}"))).collect();
    let thousands = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counting = thousands.clone();
    store.db.progress_handler(1000, Some(move || counting.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == u32::MAX)).unwrap();

    let exchanged = store.exchange(&Exchange { addressees: vec![Addressee { name: "proxy".into(), taken, ..Addressee::default() }], ..Exchange::default() }).unwrap();

    assert_eq!(exchanged.rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["m-03990", "m-03991", "m-03992", "m-03993", "m-03994", "m-03995", "m-03996", "m-03997", "m-03998", "m-03999"]);
    let steps = thousands.load(std::sync::atomic::Ordering::Relaxed);
    assert!(steps < 500, "the exchange took {steps} thousand SQLite steps");
}

#[test]
fn a_spooled_row_that_does_not_parse_or_that_a_sessions_own_location_holds_under_another_sender_for_another_session_or_under_a_pending_id_is_set_aside_and_the_rest_flush() {
    let root = tempfile::tempdir().unwrap();
    let spool = Spool::at(root.path());
    spool.put(&message("m-1", "sc-1")).unwrap();
    std::fs::write(root.path().join("spool/m-0.json"), r#"{"id":"m-0","kind":"MESSAGE","to_node":"sc-1","sender":"two words"}"#).unwrap();
    let own = FleetRow { sender: Sender::Session("w-1".into()), ..message("m-2", "sc-1") };
    Spool::of(root.path(), "w-1").unwrap().put(&own).unwrap();
    let forged = [
        ("m-3", FleetRow { sender: Sender::Session("boss".into()), ..message("m-3", "sc-1") }),
        ("a-4", FleetRow { kind: RowKind::GateAnswer, answer: Some(AnswerPayload { gate: 7, number: 1 }), ..message("a-4", "sc-1") }),
        ("j-5", FleetRow { kind: RowKind::JobExit, sender: Sender::Service("north-job-run".into()), ..message("j-5", "sc-1") }),
        ("g-6", FleetRow { kind: RowKind::Gate, sender: Sender::Session("w-1".into()), ..message("g-6", "sc-1") }),
        ("m-1", FleetRow { sender: Sender::Session("w-1".into()), ..message("m-1", "sc-1") }),
    ];
    for (id, row) in &forged {
        std::fs::write(root.path().join(format!("spool/sessions/w-1/{id}.json")), serde_json::to_vec(row).unwrap()).unwrap();
    }

    let listed: Vec<String> = spool.listing().unwrap().into_iter().map(|spooled| format!("{} {}", spooled.id, spooled.row["spooled_by"])).collect();
    assert_eq!(listed, ["m-0 null", "m-1 null", "m-2 \"w-1\""]);
    assert_eq!(spool.pending().unwrap(), [message("m-1", "sc-1"), FleetRow { spooled_by: Some("w-1".into()), ..own }]);
    for id in ["m-0", "m-3", "a-4", "j-5", "g-6", "m-1"] {
        assert!(root.path().join(format!("spool/rejected/{id}.json")).is_file(), "{id} was not set aside");
    }
    assert!(!root.path().join("spool/m-0.json").exists());
}


#[test]
fn a_stopped_session_still_receives_the_answer_to_its_open_gate_but_no_message() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let answer = FleetRow { kind: RowKind::GateAnswer, answer: Some(AnswerPayload { gate: 22, number: 1 }), ..message("a-1", "sc-1") };
    store.put(&[message("m-1", "sc-1"), answer]).unwrap();

    let exchanged = store.exchange(&Exchange { addressees: vec![Addressee { name: "sc-1".into(), stopped: true, ..Addressee::default() }], ..Exchange::default() }).unwrap();

    assert_eq!(exchanged.rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["a-1"]);
}

#[test]
fn a_launch_that_died_before_its_session_registered_leaves_its_name_to_a_later_fresh_claim() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let died = SessionRow { updated: "2026-10-31T04:27:00Z".into(), ..session("safety-lead-2", "nexus", "starting") };
    store.register(&died, Claim::Fresh).unwrap();

    store.register(&session("safety-lead-2", "whiterabbit", "starting"), Claim::Fresh).unwrap();
    let Err(NorthError::Refused(held)) = store.register(&session("safety-lead-2", "nexus", "starting"), Claim::Fresh) else { panic!("a live name was claimed twice") };

    let hosts: Vec<String> = store.sessions().unwrap().into_iter().map(|row| row.host).collect();
    assert_eq!((held.as_str(), hosts), ("the session name safety-lead-2 is held fleet-wide by a starting session on whiterabbit", vec!["whiterabbit".to_owned()]));
}

#[test]
fn a_settled_wait_leaves_its_registration_in_the_wait_history_and_a_ledger_from_before_the_runtime_kind_keeps_its_waits() {
    let (_directory, path) = ledger();
    drop(Store::open(&path).unwrap());
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TRIGGER fleet_waits_settled; DROP TABLE fleet_waits; CREATE TABLE fleet_waits(session TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('job','farm','branch','capture')), key TEXT NOT NULL, deadline TEXT NOT NULL, created TEXT NOT NULL, PRIMARY KEY(session, kind, key)) STRICT; INSERT INTO fleet_waits VALUES ('held', 'branch', 'n1-fix', '2026-11-14T00:00:00Z', '2026-11-13T00:00:00Z');").unwrap();
    drop(db);
    let mut store = Store::open(&path).unwrap();
    store.wait(&WaitRequest { session: "north-730-d0351".into(), kind: WaitKind::Runtime, key: "abcd1234".into(), within_secs: 60 }).unwrap();
    store.settle("north-730-d0351", WaitKind::Runtime, "abcd1234").unwrap();

    let db = Connection::open(&path).unwrap();
    let live: Vec<String> = db.prepare("SELECT key FROM fleet_waits").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    let kept: Vec<(String, String)> = db.prepare("SELECT session, key FROM fleet_wait_history").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!((live, kept), (vec!["n1-fix".to_owned()], vec![("north-730-d0351".to_owned(), "abcd1234".to_owned())]));

    db.execute_batch("PRAGMA ignore_check_constraints = ON; INSERT INTO fleet_waits VALUES ('held', 'later', 'x', '2026-11-14T00:00:00Z', '2026-11-13T00:00:00Z');").unwrap();
    let listed: Vec<String> = store.waits().unwrap().into_iter().map(|wait| wait.key).collect();
    assert_eq!(listed, ["n1-fix"], "a wait of a kind this binary does not know is left out of the listing, not an error");
}

#[test]
fn the_exchange_after_a_host_leaves_placement_retires_the_session_rows_it_held() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.publish("ccx33", &[session("n-held", "ccx33", "working")]).unwrap();
    store.publish("gex45", &[session("n-kept", "gex45", "working")]).unwrap();
    let mut store = store.placing(Some(vec!["nexus".into(), "gex45".into()]));

    let exchanged = store.exchange(&Exchange { host: "nexus".into(), ..Exchange::default() }).unwrap();

    assert_eq!(exchanged.sessions.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(), ["n-kept"]);
}
