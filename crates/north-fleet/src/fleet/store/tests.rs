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
fn an_id_index_built_before_gate_answer_rows_existed_is_renewed_so_a_gate_answer_commits_once() {
    let (_directory, path) = ledger();
    Store::open(&path).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP INDEX events_fleet_id; CREATE UNIQUE INDEX events_fleet_id ON events(json_extract(data,'$.id')) WHERE kind IN ('MESSAGE','GATE','ANSWER','JOB-START','JOB-BEAT','JOB-EXIT');")
        .unwrap();
    let answer = FleetRow { kind: RowKind::GateAnswer, answer: Some(AnswerPayload { gate: 4, number: 1 }), ..message("a-1", "sc-1") };

    let mut store = Store::open(&path).unwrap();
    store.put(std::slice::from_ref(&answer)).unwrap();
    store.put(&[answer]).unwrap();

    assert_eq!(count(&path, "SELECT count(*) FROM events WHERE kind = 'GATE-ANSWER'"), 1);
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
    let one = br#"{"id":"m-1","kind":"MESSAGE","to":"sc-1","from":"nexus","sender":"tom","sent_at":5,"text":"hi"}"#;
    let two = br#"[{"id":"m-1","kind":"JOB-EXIT","to_node":"sc-1","from":"nexus","sender":"tom","urgent":true,"sent_at":5,"job":{"key":"j","exit":0}},{"id":"m-2","kind":"GATE","to":"sc-1","from":"nexus","sender":"tom","sent_at":6}]"#;

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

    store.register(&session("sc-1", "nexus", "working"), Claim::Fresh).unwrap();
    let Err(NorthError::Refused(why)) = store.register(&session("sc-1", "gex45", "starting"), Claim::Fresh) else { panic!("a live name was claimed twice") };

    assert_eq!(why, "the session name sc-1 is held fleet-wide by a working session on nexus");
    assert_eq!(store.sessions().unwrap(), [session("sc-1", "nexus", "working")]);
}

#[test]
fn a_launch_takes_over_the_name_of_an_exited_session() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.register(&session("sc-1", "nexus", "exited"), Claim::Fresh).unwrap();

    store.register(&session("sc-1", "gex45", "starting"), Claim::Fresh).unwrap();

    assert_eq!(store.sessions().unwrap(), [session("sc-1", "gex45", "starting")]);
}

#[test]
fn a_take_over_replaces_a_live_row_and_a_fresh_launch_replaces_a_row_its_own_host_holds() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.register(&session("lead", "nexus", "working"), Claim::Fresh).unwrap();

    store.register(&session("lead", "whiterabbit", "starting"), Claim::TakeOver).unwrap();
    store.register(&session("lead", "whiterabbit", "starting"), Claim::Fresh).unwrap();

    assert_eq!(store.sessions().unwrap(), [session("lead", "whiterabbit", "starting")]);
    assert!(matches!(store.register(&session("lead", "nexus", "starting"), Claim::Fresh), Err(NorthError::Refused(_))));
}

#[test]
fn a_host_publishes_its_registry_without_taking_a_live_name_from_another_host() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.register(&session("sc-1", "nexus", "working"), Claim::Fresh).unwrap();
    store.register(&session("sc-2", "nexus", "exited"), Claim::Fresh).unwrap();

    let held = store.publish("gex45", &[session("sc-1", "gex45", "idle"), session("sc-2", "gex45", "idle"), session("sc-3", "gex45", "parked")]).unwrap();
    let held_again = store.publish("gex45", &[session("sc-3", "gex45", "working")]).unwrap();

    assert_eq!(held, ["sc-1"]);
    assert!(held_again.is_empty());
    assert_eq!(store.sessions().unwrap(), [session("sc-1", "nexus", "working"), session("sc-3", "gex45", "working")]);
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
fn a_row_file_removed_between_listing_and_reading_is_skipped_and_a_file_this_build_cannot_decode_lists_by_its_id() {
    let root = tempfile::tempdir().unwrap();
    let spool = Spool::at(root.path());
    spool.put(&message("m-1", "sc-1")).unwrap();
    spool.put(&message("m-3", "sc-1")).unwrap();
    std::os::unix::fs::symlink(root.path().join("spool/gone"), root.path().join("spool/m-2.json")).unwrap();
    std::fs::write(root.path().join("spool/m-4.json"), br#"{"id":"m-4","kind":"ANSWER"}"#).unwrap();
    std::fs::write(root.path().join("spool/m-5.json"), b"not json").unwrap();

    let ids: Vec<String> = spool.listing().unwrap().into_iter().map(|spooled| spooled.id).collect();

    assert_eq!(ids, ["m-1", "m-3", "m-4", "m-5"]);
    assert_eq!(spool.pending().unwrap(), [message("m-1", "sc-1"), message("m-3", "sc-1")], "rows this build cannot read are set aside, not flushed");
    assert!(root.path().join("spool/rejected/m-4.json").is_file() && root.path().join("spool/rejected/m-5.json").is_file());
    assert_eq!(spool.listing().unwrap().into_iter().map(|spooled| spooled.id).collect::<Vec<_>>(), ["m-1", "m-3"]);
}

#[test]
fn the_cached_session_table_round_trips_and_a_host_without_one_knows_no_sessions() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(cached_sessions(root.path()).unwrap(), None);
    cache_sessions(root.path(), &[session("sc-1", "nexus", "working")]).unwrap();
    cache_sessions(root.path(), &[session("sc-2", "gex45", "idle"), session("sc-1", "nexus", "working")]).unwrap();

    assert_eq!(cached_sessions(root.path()).unwrap(), Some(vec![session("sc-2", "gex45", "idle"), session("sc-1", "nexus", "working")]));
}

#[test]
fn a_published_session_keeps_the_columns_the_fleet_list_shows() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let detailed = SessionRow {
        provider: "claude".into(),
        account: "b".into(),
        cwd: "/home/tom/code/north/main".into(),
        parent: Some("lead".into()),
        placement: Some("gex45".into()),
        machine: Some("gex45".into()),
        tasks: vec!["north#429".into(), "north#316".into()],
        conversation: Some("7c1e0f6a-conversation".into()),
        ..session("sc-1", "nexus", "working")
    };

    store.publish("nexus", std::slice::from_ref(&detailed)).unwrap();

    assert_eq!(store.sessions().unwrap(), [detailed]);
}

#[test]
fn a_sessions_table_that_predates_the_list_columns_gains_them_with_its_rows_kept() {
    let (_directory, path) = ledger();
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE fleet_sessions(name TEXT PRIMARY KEY, host TEXT NOT NULL, status TEXT NOT NULL, created TEXT NOT NULL, updated TEXT NOT NULL) STRICT; INSERT INTO fleet_sessions VALUES ('old', 'nexus', 'idle', 't', 't');")
        .unwrap();

    let store = Store::open(&path).unwrap();

    assert_eq!(store.sessions().unwrap(), [session("old", "nexus", "idle")].map(|row| SessionRow { created: "t".into(), updated: "t".into(), ..row }));
}

fn job_row(id: &str, kind: RowKind, key: &str, lease_secs: Option<u64>, exit: Option<i32>) -> FleetRow {
    FleetRow { kind, job: Some(JobPayload { key: key.into(), lease_secs, exit }), ..message(id, "sc-1") }
}

fn seconds_between(from: &str, to: &str) -> i64 {
    Timestamp::parse(to).unwrap().unix_seconds() - Timestamp::parse(from).unwrap().unix_seconds()
}

#[test]
fn a_job_start_registers_a_job_wait_with_its_lease_and_each_beat_extends_it_and_the_exit_settles_it() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();

    store.put_at(&[job_row("j-1", RowKind::JobStart, "soak@nexus#7", Some(180), None)], Timestamp::from_unix_seconds(1_793_000_000)).unwrap();
    let [started] = store.waits().unwrap().try_into().unwrap();
    assert_eq!((started.session.as_str(), started.kind, started.key.as_str()), ("sc-1", WaitKind::Job, "soak@nexus#7"));
    assert_eq!(seconds_between(&started.created, &started.deadline), 180);

    store.put_at(&[job_row("j-2", RowKind::JobBeat, "soak@nexus#7", Some(600), None)], Timestamp::from_unix_seconds(1_793_000_007)).unwrap();
    let [beaten] = store.waits().unwrap().try_into().unwrap();
    assert_eq!(seconds_between(&beaten.created, &beaten.deadline), 7 + 600);
    assert_eq!(beaten.created, started.created);

    store.put_at(&[job_row("j-3", RowKind::JobExit, "soak@nexus#7", None, Some(3))], Timestamp::from_unix_seconds(1_793_000_009)).unwrap();
    assert!(store.waits().unwrap().is_empty());
    assert_eq!(count(&path, "SELECT count(*) FROM events WHERE kind IN ('JOB-START','JOB-BEAT','JOB-EXIT')"), 3);
}

#[test]
fn a_session_is_leased_to_a_job_only_from_its_job_start_until_its_exit_or_its_lease_lapses() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    assert!(!store.job_leased("sc-1").unwrap());

    store.put(&[job_row("j-1", RowKind::JobStart, "soak#1", Some(180), None)]).unwrap();
    assert!(store.job_leased("sc-1").unwrap());
    assert!(!store.job_leased("sc-2").unwrap());

    store.put(&[job_row("j-2", RowKind::JobExit, "soak#1", None, Some(0))]).unwrap();
    assert!(!store.job_leased("sc-1").unwrap());

    store.put(&[job_row("j-3", RowKind::JobStart, "gone#2", Some(0), None)]).unwrap();
    assert!(!store.job_leased("sc-1").unwrap(), "a lease that ran out with no beat is not a wait");
    store.wait(&WaitRequest { session: "sc-1".into(), kind: WaitKind::Farm, key: "42".into(), within_secs: 3600 }).unwrap();
    assert!(!store.job_leased("sc-1").unwrap(), "only a job wait is a job lease");
}

#[test]
fn a_job_start_without_a_lease_takes_the_default_and_a_late_beat_or_a_repeated_start_changes_nothing() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();

    store.put(&[job_row("j-1", RowKind::JobBeat, "late#1", Some(180), None)]).unwrap();
    assert!(store.waits().unwrap().is_empty(), "a beat with no start registers nothing");

    store.put(&[job_row("j-2", RowKind::JobStart, "soak#1", None, None)]).unwrap();
    let [first] = store.waits().unwrap().try_into().unwrap();
    assert_eq!(seconds_between(&first.created, &first.deadline), JOB_LEASE_SECS as i64);

    store.put(&[job_row("j-3", RowKind::JobExit, "soak#1", None, Some(0))]).unwrap();
    store.put(&[job_row("j-2", RowKind::JobStart, "soak#1", None, None)]).unwrap();
    assert!(store.waits().unwrap().is_empty(), "a start committed once is not replayed after its exit");
}

#[test]
fn a_probe_wait_is_one_row_per_session_kind_and_key_and_a_second_registration_moves_its_deadline() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let request = |session: &str, kind, key: &str, within_secs| WaitRequest { session: session.into(), kind, key: key.into(), within_secs };

    store.wait(&request("sc-1", WaitKind::Farm, "42", 3600)).unwrap();
    store.wait(&request("sc-1", WaitKind::Farm, "42", 7200)).unwrap();
    store.wait(&request("sc-1", WaitKind::Branch, "42", 3600)).unwrap();
    store.wait(&request("sc-2", WaitKind::Farm, "42", 3600)).unwrap();

    let waits = store.waits().unwrap();
    assert_eq!(waits.len(), 3);
    let farm = waits.iter().find(|wait| wait.session == "sc-1" && wait.kind == WaitKind::Farm).unwrap();
    assert_eq!(seconds_between(&farm.created, &farm.deadline), 7200);
    assert!(store.settle("sc-1", WaitKind::Farm, "42").unwrap());
    assert!(!store.settle("sc-1", WaitKind::Farm, "42").unwrap());
    assert_eq!(store.waits().unwrap().len(), 2);
}

#[test]
fn a_wait_request_needs_a_session_and_a_key_and_a_known_kind() {
    let good = br#"{"session":"sc-1","kind":"capture","key":"/tmp/done","within_secs":60}"#;
    assert_eq!(WaitRequest::parse(good).unwrap().kind, WaitKind::Capture);
    assert!(WaitRequest::parse(br#"{"session":"","kind":"farm","key":"1","within_secs":60}"#).is_err());
    assert!(WaitRequest::parse(br#"{"session":"sc-1","kind":"soak","key":"1","within_secs":60}"#).is_err());
}

#[test]
fn a_publish_leaves_a_row_still_launching_on_its_host_and_removes_one_whose_launch_went_stale() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let now = Timestamp::now().iso();
    store.register(&SessionRow { updated: now.clone(), ..session("fresh", "gex45", "starting") }, Claim::Fresh).unwrap();
    store.register(&session("stale", "gex45", "starting"), Claim::Fresh).unwrap();

    store.publish("gex45", &[]).unwrap();

    assert_eq!(store.sessions().unwrap(), [SessionRow { updated: now, ..session("fresh", "gex45", "starting") }]);
}

#[test]
fn a_job_exit_that_reached_the_store_over_a_day_ago_is_not_delivered_and_a_fresh_one_is() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let exit = |id: &str| FleetRow { kind: RowKind::JobExit, job: Some(JobPayload { key: "soak".into(), lease_secs: None, exit: Some(0) }), ..message(id, "sc-1") };
    store.put(&[exit("x-old"), exit("x-new")]).unwrap();
    Connection::open(&path).unwrap().execute("UPDATE events SET ts = '2026-01-01T00:00:00Z' WHERE json_extract(data, '$.id') = 'x-old'", []).unwrap();

    let exchanged = store.exchange(&Exchange { addressees: vec![Addressee { name: "sc-1".into(), ..Addressee::default() }], ..Exchange::default() }).unwrap();

    assert_eq!(exchanged.rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["x-new"]);
}

fn wait_at(store: &mut Store, request: &WaitRequest, unix_seconds: i64) {
    let tx = store.db.transaction().unwrap();
    waits::insert(&tx, request, Timestamp::from_unix_seconds(unix_seconds)).unwrap();
    tx.commit().unwrap();
}

#[test]
fn a_settlement_decided_on_a_wait_does_not_remove_the_wait_a_worker_registered_again_under_its_key() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let request = WaitRequest { session: "sc-1".into(), kind: WaitKind::Farm, key: "42".into(), within_secs: 60 };
    wait_at(&mut store, &request, 1_793_000_000);
    let judged = store.waits().unwrap()[0].key();
    wait_at(&mut store, &request, 1_793_000_001);

    store.settle_all(std::slice::from_ref(&judged)).unwrap();

    assert_eq!(store.waits().unwrap().len(), 1, "the settle named the earlier registration");
    let current = store.waits().unwrap()[0].key();
    store.settle_all(&[current]).unwrap();
    assert!(store.waits().unwrap().is_empty());
}

#[test]
fn a_session_gone_from_its_hosts_registry_loses_its_fleet_sessions_row_so_its_name_is_free() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    store.publish("nexus", &[session("kept", "nexus", "idle"), session("forgotten", "nexus", "idle")]).unwrap();
    store.publish("gex45", &[session("far", "gex45", "idle")]).unwrap();

    store.publish("nexus", &[session("kept", "nexus", "idle")]).unwrap();

    let names: Vec<String> = store.sessions().unwrap().into_iter().map(|row| row.name).collect();
    assert_eq!(names, ["far", "kept"]);
}

fn gate_row(host: &str, id: i64, issue: Option<&str>) -> GateRow {
    let request = crate::fleet::gates::GateRequest { verification: false, issue: issue.map(str::to_owned), box_text: None, session: "probe".into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 600_000 };
    GateRow { host: host.into(), id, request, opened_ms: 1_793_000_000_000 }
}

#[test]
fn a_hosts_published_gates_replace_its_earlier_ones_so_a_closed_gate_disappears_and_other_hosts_stay() {
    let (_directory, path) = ledger();
    let mut store = Store::open(&path).unwrap();
    let exchange = |host: &str, gates: Vec<GateRow>| Exchange { host: host.into(), gates, ..Exchange::default() };

    store.exchange(&exchange("nexus", vec![gate_row("nexus", 4, Some("north#416")), gate_row("nexus", 5, None)])).unwrap();
    store.exchange(&exchange("gex45", vec![gate_row("gex45", 1, None)])).unwrap();
    let closed = store.exchange(&exchange("nexus", vec![gate_row("nexus", 5, None)])).unwrap();

    let held: Vec<(String, i64)> = closed.gates.iter().map(|gate| (gate.host.clone(), gate.id)).collect();
    assert_eq!(held, [("gex45".to_owned(), 1), ("nexus".to_owned(), 5)]);
    store.exchange(&exchange("nexus", vec![gate_row("gex45", 9, None)])).unwrap();
    assert_eq!(store.gates().unwrap().iter().filter(|gate| gate.host == "nexus").count(), 0, "a host publishes only its own gates");
}

#[test]
fn the_cached_fleet_table_round_trips_its_gates_and_a_host_without_one_knows_none() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(cached_gates(root.path()).unwrap(), []);

    let table = FleetTable { sessions: vec![session("sc-1", "nexus", "working")], gates: vec![gate_row("nexus", 4, None)] };
    cache_table(root.path(), &table).unwrap();

    assert_eq!(cached_gates(root.path()).unwrap(), table.gates);
    assert_eq!(cached_sessions(root.path()).unwrap(), Some(table.sessions));
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
fn a_row_stored_before_w1_reads_a_bare_name_as_that_session_and_a_missing_sender_as_unknown() {
    let mut bare = serde_json::to_value(message("m-1", "sc-1")).unwrap();
    bare["sender"] = "895929a7-6218-4168-b513-3dced3440bf6".into();
    let mut absent = serde_json::to_value(message("m-2", "sc-1")).unwrap();
    absent.as_object_mut().unwrap().remove("sender");

    assert_eq!(serde_json::from_value::<FleetRow>(bare).unwrap().sender, crate::fleet::sender::Sender::Session("895929a7-6218-4168-b513-3dced3440bf6".into()));
    assert_eq!(serde_json::from_value::<FleetRow>(absent).unwrap().sender, crate::fleet::sender::Sender::unknown());
    assert!("lead".parse::<crate::fleet::sender::Sender>().is_err(), "the CLI still refuses a bare name");
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

#[test]
fn rows_addressed_to_sessions_are_found_through_the_to_node_index_rather_than_a_scan_of_every_event() {
    let (_directory, path) = ledger();
    let store = Store::open(&path).unwrap();
    let plan: Vec<String> = store.db.prepare(&format!("EXPLAIN QUERY PLAN {}", exchange::ADDRESSED)).unwrap().query_map([rusqlite::types::Null; 5], |row| row.get(3)).unwrap().collect::<Result<_, _>>().unwrap();

    assert!(plan.iter().any(|step| step.contains("USING INDEX events_fleet_to")), "{plan:?}");
}
