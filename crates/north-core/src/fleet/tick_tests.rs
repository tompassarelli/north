use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::error::NorthError;
use crate::fleet::runtime::Runtime;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::store::{Exchanged, FarSpools, FleetRow, RowKind, Store};
use crate::providers::claude::fleet::Claude;

const EVENTS: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL CHECK(kind IN ('KICKOFF')), actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;";
const CREATED_MS: u64 = 1_793_000_000_000;
const NOW_MS: u64 = CREATED_MS + 60_000;
const BUDGET: Duration = Duration::from_secs(20);

struct Host {
    home: tempfile::TempDir,
    fleet: Fleet,
    ledger: PathBuf,
}

fn host(name: &str) -> Host {
    let home = tempfile::tempdir().unwrap();
    let books = Arc::new(crate::ledger::Ledger::open(&home.path().join("ledger.db")).unwrap());
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), books).unwrap().hosted_as(name);
    let ledger = home.path().join("threads.db");
    rusqlite::Connection::open(&ledger).unwrap().execute_batch(EVENTS).unwrap();
    Host { home, fleet, ledger }
}

impl Host {
    fn root(&self) -> PathBuf {
        self.home.path().join(".local/state/north/fleet")
    }

    fn store(&self) -> Store {
        Store::open(&self.ledger).unwrap()
    }

    fn pulls(&self) -> Pulls<'static> {
        Pulls { spools: &Nobody, always: Vec::new() }
    }

    fn tick(&self) -> Ticked {
        tick(&self.fleet, &mut self.store(), &self.pulls(), NOW_MS, BUDGET).unwrap()
    }
}

fn live(host: &Host, name: &str) -> Arc<Mutex<Vec<String>>> {
    let dir = host.root().join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": CREATED_MS, "host_pid": std::process::id(), "conversation": name, "host_dir": dir, "stream_offset": 0});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let assistant = json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": "Not done: waiting."}]}});
    let result = json!({"type": "result", "subtype": "success", "session_id": name});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{assistant}\n{result}\n")).unwrap();
    let listener = UnixListener::bind(dir.join("host.sock")).unwrap();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let seen = frames.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut writer = &stream;
            for line in BufReader::new(&stream).lines().map_while(Result::ok) {
                seen.lock().unwrap().push(line);
                writeln!(writer, "{{\"host_seq\":1}}").unwrap();
            }
        }
    });
    frames
}

fn message(id: &str, to: &str, sent_at: u64) -> FleetRow {
    FleetRow { id: id.into(), kind: RowKind::Message, to: to.into(), from: "whiterabbit".into(), urgent: false, sent_at, text: "hello".into(), sender: Some("lead".into()), gate: None, job: None }
}

fn count(ledger: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(ledger).unwrap().query_row(sql, [], |row| row.get(0)).unwrap()
}

fn frame_uuids(frames: &Mutex<Vec<String>>) -> Vec<String> {
    frames.lock().unwrap().iter().map(|line| serde_json::from_str::<Value>(line).unwrap()["uuid"].as_str().unwrap().to_owned()).collect()
}

struct Unreachable;

impl LedgerHost for Unreachable {
    fn exchange(&mut self, _: &Exchange) -> NorthResult<Exchanged> {
        Err(NorthError::Refused("ssh nexus: timed out; remote outcome unknown".into()))
    }
}

#[test]
fn a_spooled_row_stays_spooled_until_the_ledger_host_commits_it_and_is_dropped_after() {
    let host = host("whiterabbit");
    let row = message("m-1", "elsewhere", NOW_MS);
    host.fleet.spool().put(&row).unwrap();

    assert!(tick(&host.fleet, &mut Unreachable, &host.pulls(), NOW_MS, BUDGET).is_err());
    assert_eq!(host.fleet.spool().pending().unwrap(), std::slice::from_ref(&row), "a row whose commit is unknown stays in the spool");

    let ticked = host.tick();

    assert_eq!(ticked.flushed, 1);
    assert!(host.fleet.spool().pending().unwrap().is_empty());
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-1'"), 1);
}

#[test]
fn a_tick_publishes_this_hosts_sessions_and_caches_the_fleet_table() {
    let host = host("nexus");
    let _frames = live(&host, "worker");
    host.store().publish(&[SessionRow { name: "far".into(), host: "gex45".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }]).unwrap();

    host.tick();

    let cached = crate::fleet::store::cached_sessions(&host.root()).unwrap().unwrap();
    assert_eq!(cached.iter().map(|row| (row.name.as_str(), row.host.as_str())).collect::<Vec<_>>(), [("far", "gex45"), ("worker", "nexus")]);
}

#[test]
fn a_row_already_in_the_sessions_journal_is_not_delivered_again_by_a_second_tick() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    host.store().put(&[message("m-1", "worker", NOW_MS)]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!(first.delivered, [("m-1".to_owned(), "worker".to_owned())]);
    assert_eq!((second.delivered.len(), second.already), (0, 0), "a journaled id is not fetched again");
    assert_eq!(host.fleet.hand_in(&message("m-1", "worker", NOW_MS)).unwrap(), HandedIn::Already);
    assert_eq!(frames.lock().unwrap().len(), 1);
    assert!(frames.lock().unwrap()[0].contains("[from lead@whiterabbit] hello"), "a row from another host is signed with its host");
}

#[test]
fn a_row_sent_before_the_session_was_created_is_not_delivered() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    host.store().put(&[message("m-old", "worker", CREATED_MS - 1), message("m-new", "worker", CREATED_MS)]).unwrap();

    let ticked = host.tick();

    assert_eq!(ticked.delivered, [("m-new".to_owned(), "worker".to_owned())]);
    assert_eq!(frames.lock().unwrap().len(), 1);
}

#[test]
fn a_session_that_took_its_name_over_gets_the_rows_its_predecessor_never_took_and_none_older() {
    let host = host("nexus");
    let frames = live(&host, "lead");
    let record = host.root().join("lead/session.json");
    let mut held: Value = serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    held["addressed_since_ms"] = json!(CREATED_MS - 10);
    std::fs::write(&record, held.to_string()).unwrap();
    host.store().put(&[message("m-before-both", "lead", CREATED_MS - 11), message("m-to-predecessor", "lead", CREATED_MS - 1)]).unwrap();

    let ticked = host.tick();

    assert_eq!(ticked.delivered, [("m-to-predecessor".to_owned(), "lead".to_owned())]);
    assert_eq!(frames.lock().unwrap().len(), 1);
}

#[test]
fn the_prompt_carries_the_rows_id_and_the_cli_echo_of_its_signed_text_is_matched_by_that_id() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    let row = message("m-1", "worker", NOW_MS);
    host.store().put(std::slice::from_ref(&row)).unwrap();

    host.tick();

    assert_eq!(frame_uuids(&frames), [row.prompt_id()]);
    let echo = json!({"type": "user", "isReplay": true, "uuid": row.prompt_id(), "message": {"role": "user", "content": "[from lead@whiterabbit] hello"}});
    assert_eq!(Claude.accepted_prompt(&echo), Some(row.prompt_id()));
    assert_ne!(message("m-2", "worker", NOW_MS).prompt_id(), row.prompt_id());
}

#[test]
fn a_row_from_this_host_is_signed_by_its_sender_alone() {
    let host = host("whiterabbit");
    let frames = live(&host, "worker");
    host.store().put(&[message("m-1", "worker", NOW_MS)]).unwrap();

    host.tick();

    assert!(frames.lock().unwrap()[0].contains("[from lead] hello"), "{:?}", frames.lock().unwrap());
}

#[test]
fn a_hand_in_cut_off_before_its_prompt_reached_the_session_is_delivered_once_by_the_next_tick() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    let journal = host.root().join("worker/delivered");
    std::fs::create_dir_all(&journal).unwrap();
    for id in ["m-lost", "m-taken"] {
        std::fs::write(journal.join(id), "").unwrap();
        std::fs::write(journal.join(format!("{id}.handing")), "").unwrap();
    }
    let taken = message("m-taken", "worker", NOW_MS);
    let unaccepted = json!({"id": taken.prompt_id(), "line": "frame", "replays": 0});
    std::fs::write(host.root().join("worker/unaccepted.jsonl"), format!("{unaccepted}\n")).unwrap();
    host.store().put(&[message("m-lost", "worker", NOW_MS), taken]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!(first.delivered, [("m-lost".to_owned(), "worker".to_owned())]);
    assert_eq!(first.already, 1, "a prompt the session host journaled is left to its own replay");
    assert_eq!((second.delivered.len(), second.already), (0, 0));
    assert_eq!(frame_uuids(&frames), [message("m-lost", "worker", NOW_MS).prompt_id()]);
    assert!(!journal.join("m-lost.handing").exists() && !journal.join("m-taken.handing").exists());
}

struct Nobody;

impl FarSpools for Nobody {
    fn take(&self, ssh: &str) -> NorthResult<Vec<FleetRow>> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn drop(&self, ssh: &str, _: &[String]) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }
}

#[derive(Default)]
struct Far {
    spools: Mutex<BTreeMap<String, Vec<FleetRow>>>,
    down: Mutex<Vec<String>>,
    undroppable: Mutex<Vec<String>>,
    taken: Mutex<Vec<String>>,
}

impl Far {
    fn holding(ssh: &str, rows: Vec<FleetRow>) -> Self {
        let far = Self::default();
        far.spools.lock().unwrap().insert(ssh.into(), rows);
        far
    }

    fn left_on(&self, ssh: &str) -> Vec<String> {
        self.spools.lock().unwrap().get(ssh).map(|rows| rows.iter().map(|row| row.id.clone()).collect()).unwrap_or_default()
    }
}

impl FarSpools for Far {
    fn take(&self, ssh: &str) -> NorthResult<Vec<FleetRow>> {
        self.taken.lock().unwrap().push(ssh.into());
        if self.down.lock().unwrap().iter().any(|down| down == ssh) {
            return Err(NorthError::Refused(format!("ssh {ssh}: connection timed out")));
        }
        Ok(self.spools.lock().unwrap().get(ssh).cloned().unwrap_or_default())
    }

    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        if self.undroppable.lock().unwrap().iter().any(|down| down == ssh) {
            return Err(NorthError::Refused(format!("ssh {ssh}: connection reset")));
        }
        self.spools.lock().unwrap().entry(ssh.into()).or_default().retain(|row| !ids.contains(&row.id));
        Ok(())
    }
}

fn far_host(name: &str) -> FarHost {
    FarHost { name: name.into(), ssh: format!("tom@{name}") }
}

fn place(host: &Host, session: &str, machine: &str, stopped: bool) {
    let _frames = live(host, session);
    let path = host.root().join(session).join("session.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["machine"] = json!({"name": machine, "ssh": format!("tom@{machine}")});
    record["stopped"] = json!(stopped);
    std::fs::write(path, record.to_string()).unwrap();
}

fn ledger_ids(host: &Host) -> i64 {
    count(&host.ledger, "SELECT count(*) FROM events")
}

#[test]
fn a_row_pulled_from_a_far_spool_is_dropped_there_only_after_the_ledger_host_commits_it() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![message("m-far", "elsewhere", NOW_MS)]);
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    assert!(tick(&host.fleet, &mut Unreachable, &pulls, NOW_MS, BUDGET).is_err());
    assert_eq!(far.left_on("tom@gex45"), ["m-far"], "a row whose commit is unknown stays on its machine");

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, BUDGET).unwrap();

    assert_eq!(ticked.pulled, 1);
    assert!(ticked.pull_failed.is_empty());
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-far'"), 1);
}

#[test]
fn a_failed_pull_is_recorded_for_its_machine_alone_and_the_others_are_still_pulled() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![message("m-1", "elsewhere", NOW_MS)]);
    far.spools.lock().unwrap().insert("tom@bunker".into(), vec![message("m-2", "elsewhere", NOW_MS)]);
    far.down.lock().unwrap().push("tom@bunker".into());
    let pulls = Pulls { spools: &far, always: vec![far_host("bunker"), far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, BUDGET).unwrap();

    assert_eq!(ticked.pull_failed.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["bunker"]);
    assert_eq!(far.left_on("tom@bunker"), ["m-2"]);
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(ledger_ids(&host), 1);
    assert!(ticked.lines().iter().any(|line| line.starts_with("not pulled from bunker: ")));
}

#[test]
fn a_drop_that_fails_is_recorded_and_the_next_pull_commits_the_same_row_once() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![message("m-1", "elsewhere", NOW_MS)]);
    far.undroppable.lock().unwrap().push("tom@gex45".into());
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    let first = tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, BUDGET).unwrap();
    far.undroppable.lock().unwrap().clear();
    let second = tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, BUDGET).unwrap();

    assert_eq!(first.pull_failed.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["gex45"]);
    assert!(second.pull_failed.is_empty());
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(ledger_ids(&host), 1);
}

#[test]
fn a_row_the_store_would_refuse_stays_on_its_machine_and_does_not_hold_back_the_rest() {
    let host = host("nexus");
    let mut bad = message("m-bad", "elsewhere", NOW_MS);
    bad.from = String::new();
    let far = Far::holding("tom@gex45", vec![bad, message("m-ok", "elsewhere", NOW_MS)]);
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, BUDGET).unwrap();

    assert_eq!(far.left_on("tom@gex45"), ["m-bad"]);
    assert_eq!(ticked.pull_failed.len(), 1);
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-ok'"), 1);
}

#[test]
fn the_machines_pulled_are_the_live_sessions_placements_and_the_configured_hosts_without_this_one() {
    let host = host("nexus");
    place(&host, "on-outpost", "outpost", false);
    place(&host, "on-bunker", "bunker", true);
    place(&host, "on-nexus", "nexus", false);
    let far = Far::default();
    let pulls = Pulls { spools: &far, always: vec![far_host("annex"), far_host("nexus")] };

    tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, BUDGET).unwrap();

    let mut taken = far.taken.lock().unwrap().clone();
    taken.sort();
    assert_eq!(taken, ["tom@annex", "tom@outpost"]);
}

struct Slow(Duration);

impl FarSpools for Slow {
    fn take(&self, _: &str) -> NorthResult<Vec<FleetRow>> {
        std::thread::sleep(self.0);
        Ok(Vec::new())
    }

    fn drop(&self, _: &str, _: &[String]) -> NorthResult<()> {
        Ok(())
    }
}

#[test]
fn a_slow_far_pull_does_not_use_up_the_budget_for_delivering_to_this_hosts_sessions() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    host.store().put(&[message("m-1", "worker", NOW_MS)]).unwrap();
    let slow = Slow(Duration::from_millis(400));
    let pulls = Pulls { spools: &slow, always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, NOW_MS, Duration::from_millis(200)).unwrap();

    assert_eq!(ticked.delivered, [("m-1".to_owned(), "worker".to_owned())]);
    assert_eq!(ticked.deferred, 0);
    assert_eq!(frame_uuids(&frames).len(), 1);
}
