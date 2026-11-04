use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::error::NorthError;
use crate::fleet::gates::GateRequest;
use crate::fleet::runtime::Runtime;
use crate::fleet::runtimes::Runtimes;
use crate::fleet::registry::Controlled;
use crate::fleet::store::{AnswerPayload, ControlVerb, Exchanged, FarRegistry, FarSpools, FleetRow, FleetTable, HandedRow, JobPayload, Pull, RowKind, SessionRow, Store, Undecodable, UnknownKind, WaitKey};
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
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Claude)]), books).unwrap().hosted_as(name).arming(|_| Ok(()));
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
        tick(&self.fleet, &mut self.store(), &self.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap()
    }
}

fn live(host: &Host, name: &str) -> Arc<Mutex<Vec<String>>> {
    live_with(host, name, "Not done: waiting.", None)
}

fn live_with(host: &Host, name: &str, reply: &str, task_ref: Option<&str>) -> Arc<Mutex<Vec<String>>> {
    let dir = host.root().join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let mut record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": CREATED_MS, "host_pid": std::process::id(), "conversation": name, "host_dir": dir, "stream_offset": 0});
    record["task_ref"] = json!(task_ref);
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let assistant = json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": reply}]}});
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
    FleetRow { id: id.into(), kind: RowKind::Message, to: to.into(), from: "whiterabbit".into(), urgent: false, sent_at, text: "hello".into(), sender: Some("lead".into()), gate: None, answer: None, job: None, origin: None }
}

fn far_message(id: &str, machine: &str) -> FleetRow {
    FleetRow { from: machine.into(), sender: None, ..message(id, "elsewhere", NOW_MS) }
}

fn gate(id: &str, to: &str) -> FleetRow {
    let request = GateRequest { verification: false, issue: None, session: to.into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 60_000 };
    FleetRow { kind: RowKind::Gate, text: request.question.clone(), sender: None, gate: Some(Box::new(request)), ..message(id, to, CREATED_MS) }
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

    fn settle(&mut self, _: &[WaitKey]) -> NorthResult<()> {
        Err(NorthError::Refused("ssh nexus: timed out; remote outcome unknown".into()))
    }
}

#[test]
fn a_spooled_row_stays_spooled_until_the_ledger_host_commits_it_and_is_dropped_after() {
    let host = host("whiterabbit");
    let row = message("m-1", "elsewhere", NOW_MS);
    host.fleet.spool().put(&row).unwrap();

    assert!(tick(&host.fleet, &mut Unreachable, &host.pulls(), &NoProbes, NOW_MS, BUDGET).is_err());
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
    host.store().publish("gex45", &[SessionRow { name: "far".into(), host: "gex45".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }]).unwrap();

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
    fn take(&self, ssh: &str) -> NorthResult<Pull> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn drop(&self, ssh: &str, _: &[String]) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn set_aside(&self, ssh: &str, _: &[String]) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn cache(&self, ssh: &str, _: &FleetTable) -> NorthResult<()> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn registry(&self, ssh: &str) -> NorthResult<FarRegistry> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }

    fn deliver(&self, ssh: &str, _: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Err(NorthError::Refused(format!("ssh {ssh}: nobody is pulled from")))
    }
}

#[derive(Default)]
struct Far {
    spools: Mutex<BTreeMap<String, Vec<FleetRow>>>,
    down: Mutex<Vec<String>>,
    undroppable: Mutex<Vec<String>>,
    undecodable: Mutex<BTreeMap<String, Vec<String>>>,
    newer: Mutex<BTreeMap<String, Vec<String>>>,
    taken: Mutex<Vec<String>>,
    aside: Mutex<Vec<String>>,
    cached: Mutex<BTreeMap<String, Vec<String>>>,
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
    fn take(&self, ssh: &str) -> NorthResult<Pull> {
        self.taken.lock().unwrap().push(ssh.into());
        if self.down.lock().unwrap().iter().any(|down| down == ssh) {
            return Err(NorthError::Refused(format!("ssh {ssh}: connection timed out")));
        }
        let rows = self.spools.lock().unwrap().get(ssh).cloned().unwrap_or_default();
        let undecodable = self.undecodable.lock().unwrap().get(ssh).into_iter().flatten().map(|id| Undecodable { id: id.clone(), why: "this build cannot read it: missing field `id`".into() }).collect();
        let unknown_kind = self.newer.lock().unwrap().get(ssh).into_iter().flatten().map(|id| UnknownKind { id: id.clone(), kind: "ANSWER-V2".into() }).collect();
        Ok(Pull { rows, undecodable, unknown_kind })
    }

    fn drop(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        if self.undroppable.lock().unwrap().iter().any(|down| down == ssh) {
            return Err(NorthError::Refused(format!("ssh {ssh}: connection reset")));
        }
        self.spools.lock().unwrap().entry(ssh.into()).or_default().retain(|row| !ids.contains(&row.id));
        self.undecodable.lock().unwrap().entry(ssh.into()).or_default().retain(|id| !ids.contains(id));
        Ok(())
    }

    fn set_aside(&self, ssh: &str, ids: &[String]) -> NorthResult<()> {
        self.aside.lock().unwrap().extend(ids.iter().cloned());
        self.drop(ssh, ids)
    }

    fn cache(&self, ssh: &str, table: &FleetTable) -> NorthResult<()> {
        self.cached.lock().unwrap().insert(ssh.into(), table.sessions.iter().map(|session| session.name.clone()).collect());
        Ok(())
    }

    fn registry(&self, _: &str) -> NorthResult<FarRegistry> {
        Ok(FarRegistry::default())
    }

    fn deliver(&self, _: &str, _: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Ok(Vec::new())
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
    let far = Far::holding("tom@gex45", vec![far_message("m-far", "gex45")]);
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    assert!(tick(&host.fleet, &mut Unreachable, &pulls, &NoProbes, NOW_MS, BUDGET).is_err());
    assert_eq!(far.left_on("tom@gex45"), ["m-far"], "a row whose commit is unknown stays on its machine");

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(ticked.pulled, 1);
    assert!(ticked.pull_failed.is_empty());
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-far'"), 1);
}

#[test]
fn a_failed_pull_is_recorded_for_its_machine_alone_and_the_others_are_still_pulled() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![far_message("m-1", "gex45")]);
    far.spools.lock().unwrap().insert("tom@bunker".into(), vec![far_message("m-2", "bunker")]);
    far.down.lock().unwrap().push("tom@bunker".into());
    let pulls = Pulls { spools: &far, always: vec![far_host("bunker"), far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(ticked.pull_failed.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["bunker"]);
    assert_eq!(far.left_on("tom@bunker"), ["m-2"]);
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(ledger_ids(&host), 1);
    assert!(ticked.lines().iter().any(|line| line.starts_with("not pulled from bunker: ")));
}

#[test]
fn a_drop_that_fails_is_recorded_and_the_next_pull_commits_the_same_row_once() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![far_message("m-1", "gex45")]);
    far.undroppable.lock().unwrap().push("tom@gex45".into());
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    let first = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();
    far.undroppable.lock().unwrap().clear();
    let second = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(first.drop_failed.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["gex45"]);
    assert!(first.pull_failed.is_empty() && second.drop_failed.is_empty());
    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(ledger_ids(&host), 1);
}

#[test]
fn a_row_the_store_would_refuse_is_set_aside_on_its_machine_with_a_notice_and_does_not_hold_back_the_rest_or_count_as_a_failed_pull() {
    let host = host("nexus");
    let mut bad = far_message("m-bad", "gex45");
    bad.from = String::new();
    let far = Far::holding("tom@gex45", vec![bad, far_message("m-ok", "gex45")]);
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert!(far.left_on("tom@gex45").is_empty());
    assert_eq!(*far.aside.lock().unwrap(), ["m-bad"]);
    assert!(ticked.pull_failed.is_empty(), "{ticked:?}");
    assert!(ticked.lines().iter().any(|line| line.starts_with("set aside on gex45, the store refuses it: m-bad")), "{:?}", ticked.lines());
    let again = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();
    assert!(again.refused.is_empty(), "a row set aside never recurs");
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-ok'"), 1);
}

#[test]
fn a_row_of_a_kind_only_a_newer_build_knows_stays_in_its_spool_with_one_notice_per_tick_and_a_build_that_knows_the_kind_pulls_it() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![far_message("m-ok", "gex45")]);
    far.newer.lock().unwrap().insert("tom@gex45".into(), vec!["m-future".into()]);
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    for _ in 0..2 {
        let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

        assert!(ticked.pull_failed.is_empty() && ticked.drop_failed.is_empty() && ticked.refused.is_empty(), "{ticked:?}");
        let notices: Vec<String> = ticked.lines().into_iter().filter(|line| line.contains("m-future")).collect();
        assert_eq!(notices, ["left on gex45 for a build that knows its kind: m-future: kind ANSWER-V2 is not known to this build"]);
        assert!(far.aside.lock().unwrap().is_empty());
        assert_eq!(far.newer.lock().unwrap()["tom@gex45"], ["m-future"]);
    }
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-ok'"), 1);

    far.newer.lock().unwrap().clear();
    far.spools.lock().unwrap().insert("tom@gex45".into(), vec![far_message("m-future", "gex45")]);
    tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();
    assert_eq!(count(&host.ledger, "SELECT count(*) FROM events WHERE json_extract(data, '$.id') = 'm-future'"), 1);
}

#[test]
fn a_row_this_build_cannot_decode_is_set_aside_on_its_machine_with_a_notice_and_the_other_rows_are_still_delivered() {
    let host = host("nexus");
    let far = Far::holding("tom@gex45", vec![far_message("m-ok", "gex45")]);
    far.undecodable.lock().unwrap().insert("tom@gex45".into(), vec!["m-old-answer".into()]);
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert!(ticked.pull_failed.is_empty(), "{ticked:?}");
    assert_eq!(*far.aside.lock().unwrap(), ["m-old-answer"]);
    assert!(far.undecodable.lock().unwrap()["tom@gex45"].is_empty() && far.left_on("tom@gex45").is_empty());
    assert!(ticked.lines().iter().any(|line| line.starts_with("set aside on gex45, the store refuses it: m-old-answer: this build cannot read it")), "{:?}", ticked.lines());
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

    tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    let mut taken = far.taken.lock().unwrap().clone();
    taken.sort();
    assert_eq!(taken, ["tom@annex", "tom@outpost"]);
}

struct Slow(Duration);

impl FarSpools for Slow {
    fn take(&self, _: &str) -> NorthResult<Pull> {
        std::thread::sleep(self.0);
        Ok(Pull::default())
    }

    fn drop(&self, _: &str, _: &[String]) -> NorthResult<()> {
        Ok(())
    }

    fn set_aside(&self, _: &str, _: &[String]) -> NorthResult<()> {
        Ok(())
    }

    fn cache(&self, _: &str, _: &FleetTable) -> NorthResult<()> {
        Ok(())
    }

    fn registry(&self, _: &str) -> NorthResult<FarRegistry> {
        Ok(FarRegistry::default())
    }

    fn deliver(&self, _: &str, _: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Ok(Vec::new())
    }
}

#[test]
fn a_slow_far_pull_does_not_use_up_the_budget_for_delivering_to_this_hosts_sessions() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    host.store().put(&[message("m-1", "worker", NOW_MS)]).unwrap();
    let slow = Slow(Duration::from_millis(400));
    let pulls = Pulls { spools: &slow, always: vec![far_host("gex45")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, Duration::from_millis(200)).unwrap();

    assert_eq!(ticked.delivered, [("m-1".to_owned(), "worker".to_owned())]);
    assert_eq!(ticked.deferred, 0);
    assert_eq!(frame_uuids(&frames).len(), 1);
}

#[test]
fn a_gate_row_for_a_session_here_opens_one_gate_on_the_first_tick_and_none_on_the_next() {
    let host = host("nexus");
    let _frames = live(&host, "worker");
    host.store().put(&[gate("g-1", "worker")]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!(first.delivered, [("g-1".to_owned(), "worker".to_owned())]);
    assert_eq!((second.delivered.len(), second.already), (0, 0));
    assert_eq!(host.fleet.digest(0).unwrap().gates.len(), 1);
    assert_eq!(host.fleet.deliver_row(&gate("g-1", "worker")).unwrap(), HandedIn::Already);
}

#[test]
fn a_gate_answer_row_put_on_any_host_closes_the_gate_once_on_the_sessions_host_and_tells_the_session() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    let id = host.fleet.open_gate(gate("g-1", "worker").gate.as_deref().unwrap()).unwrap();
    let answer = FleetRow { kind: RowKind::GateAnswer, text: String::new(), sender: None, gate: None, answer: Some(AnswerPayload { gate: id, number: 2 }), ..message("a-1", "worker", NOW_MS) };
    host.store().put(&[answer]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!(first.delivered, [("a-1".to_owned(), "worker".to_owned())]);
    assert_eq!((second.delivered.len(), second.already), (0, 0));
    let closure = host.fleet.gate(id).unwrap().unwrap().closure.unwrap();
    assert_eq!(closure.answer, "no");
    assert_eq!(frames.lock().unwrap().len(), 1);
}

fn parked(host: &Host, name: &str, stopped: bool) {
    let dir = host.root().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": CREATED_MS, "host_pid": null, "conversation": name, "host_dir": dir, "stream_offset": 0, "stopped": stopped});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
}

fn cache(host: &Host, names: &[&str]) {
    let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "nexus".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
    crate::fleet::store::cache_sessions(&host.root(), &rows).unwrap();
}

fn control_rows(host: &Host) -> Vec<(String, String)> {
    host.fleet.spool().pending().unwrap().iter().map(|row| (row.kind.label().to_owned(), format!("{} {}", row.text, row.to))).collect()
}

#[test]
fn a_name_only_in_the_cached_fleet_table_turns_forget_stop_and_interrupt_into_one_spooled_control_row_each() {
    let laptop = host("laptop");
    cache(&laptop, &["worker"]);

    for verb in [ControlVerb::Forget, ControlVerb::Stop, ControlVerb::Interrupt] {
        assert!(matches!(laptop.fleet.control(verb, "worker").unwrap(), Controlled::Spooled(row) if row.kind == RowKind::Control));
    }

    let mut spooled = control_rows(&laptop);
    spooled.sort();
    assert_eq!(spooled, [("CONTROL".to_owned(), "forget worker".to_owned()), ("CONTROL".to_owned(), "interrupt worker".to_owned()), ("CONTROL".to_owned(), "stop worker".to_owned())]);
}

#[test]
fn a_name_in_neither_the_registry_nor_the_cached_table_is_refused_and_spools_nothing() {
    let laptop = host("laptop");
    cache(&laptop, &["worker"]);

    for verb in [ControlVerb::Forget, ControlVerb::Stop, ControlVerb::Interrupt] {
        assert!(matches!(laptop.fleet.control(verb, "nobody"), Err(NorthError::Usage(why)) if why == "no session named nobody"));
    }
    let uncached = host("fresh");

    assert!(matches!(uncached.fleet.control(ControlVerb::Forget, "worker"), Err(NorthError::Usage(why)) if why == "no session named worker"));
    assert!(control_rows(&laptop).is_empty() && control_rows(&uncached).is_empty());
}

#[test]
fn a_control_row_spooled_on_one_host_is_applied_once_by_the_tick_on_the_host_holding_the_session() {
    let (laptop, nexus) = (host("laptop"), host("nexus"));
    parked(&nexus, "worker", false);
    cache(&laptop, &["worker"]);
    let Controlled::Spooled(row) = laptop.fleet.control(ControlVerb::Stop, "worker").unwrap() else { panic!("a cached name is spooled") };

    assert_eq!(tick(&laptop.fleet, &mut laptop.store(), &laptop.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap().flushed, 1);
    let first = tick(&nexus.fleet, &mut laptop.store(), &nexus.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap();
    let second = tick(&nexus.fleet, &mut laptop.store(), &nexus.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(first.delivered, [(row.id.clone(), "worker".to_owned())]);
    assert_eq!(nexus.fleet.sessions().unwrap().iter().map(|record| (record.name.as_str(), record.stopped)).collect::<Vec<_>>(), [("worker", true)]);
    assert_eq!((second.delivered.len(), second.already, second.failed.len()), (0, 0, 0), "a journaled row is not fetched again");
    assert_eq!(nexus.fleet.deliver_row(&row).unwrap(), HandedIn::Already);
    assert_eq!(count(&laptop.ledger, &format!("SELECT count(*) FROM events WHERE json_extract(data, '$.id') = '{}'", row.id)), 1);
}

#[test]
fn a_forget_row_removes_a_stopped_session_from_the_holding_host_and_a_message_to_it_is_still_not_delivered() {
    let (laptop, nexus) = (host("laptop"), host("nexus"));
    parked(&nexus, "worker", true);
    cache(&laptop, &["worker"]);
    laptop.store().put(&[message("m-1", "worker", NOW_MS)]).unwrap();
    laptop.fleet.control(ControlVerb::Forget, "worker").unwrap();
    tick(&laptop.fleet, &mut laptop.store(), &laptop.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap();

    let ticked = tick(&nexus.fleet, &mut laptop.store(), &nexus.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(ticked.delivered.iter().map(|(_, to)| to.as_str()).collect::<Vec<_>>(), ["worker"], "{:?}", ticked.failed);
    assert!(nexus.fleet.sessions().unwrap().is_empty());
    let again = tick(&nexus.fleet, &mut laptop.store(), &nexus.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap();
    assert!(again.delivered.is_empty() && again.failed.is_empty());
}

fn control_row(id: &str, to: &str, verb: ControlVerb, sent_at: u64) -> FleetRow {
    FleetRow { kind: RowKind::Control, from: "laptop".into(), text: verb.label().into(), sender: None, ..message(id, to, sent_at) }
}

#[test]
fn a_control_row_sent_before_a_session_took_over_the_name_does_not_reach_the_successor() {
    let host = host("nexus");
    parked(&host, "worker", false);
    let path = host.root().join("worker/session.json");
    let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["addressed_since_ms"] = json!(CREATED_MS - 10_000);
    std::fs::write(path, record.to_string()).unwrap();
    host.store().put(&[control_row("c-old", "worker", ControlVerb::Stop, CREATED_MS - 1), message("m-old", "worker", CREATED_MS - 1)]).unwrap();
    let ticked = host.tick();

    let fetched: Vec<&str> = ticked.delivered.iter().chain(&ticked.failed).map(|(id, _)| id.as_str()).collect();

    assert_eq!(fetched, ["m-old"], "the successor takes the message from the predecessor's window and no control row from it");
    assert!(!host.fleet.sessions().unwrap()[0].stopped);
}

#[test]
fn an_interrupt_row_for_a_session_with_nothing_running_is_consumed_once_and_not_retried() {
    let host = host("nexus");
    parked(&host, "worker", true);
    host.store().put(&[control_row("c-int", "worker", ControlVerb::Interrupt, NOW_MS)]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!((first.delivered.len(), first.failed.len()), (1, 0), "{:?}", first.failed);
    assert_eq!((second.delivered.len(), second.already, second.failed.len()), (0, 0, 0));
}

struct SettleFails(Store);

impl LedgerHost for SettleFails {
    fn exchange(&mut self, exchange: &Exchange) -> NorthResult<Exchanged> {
        self.0.exchange(exchange)
    }

    fn settle(&mut self, _: &[WaitKey]) -> NorthResult<()> {
        Err(NorthError::Refused("ssh nexus: timed out; remote outcome unknown".into()))
    }
}

struct Over(WaitKey);

impl WaitProbes for Over {
    fn settle(&self, _: &Fleet) -> Probed {
        Probed { lines: Vec::new(), settled: vec![self.0.clone()] }
    }
}

#[test]
fn a_settlement_the_ledger_host_did_not_confirm_is_retried_on_the_next_tick() {
    let host = host("nexus");
    let _frames = live_with(&host, "worker", "Waiting on the build.", Some("north#429"));
    host.store().wait(&crate::fleet::store::WaitRequest { session: "worker".into(), kind: crate::fleet::store::WaitKind::Job, key: "nexus-build".into(), within_secs: 600 }).unwrap();
    let key = host.store().waits().unwrap()[0].key();

    let failed = tick(&host.fleet, &mut SettleFails(host.store()), &host.pulls(), &Over(key), NOW_MS, BUDGET).unwrap();
    assert!(failed.unsettled.is_some(), "{failed:?}");
    assert_eq!(host.store().waits().unwrap().len(), 1);

    let retried = host.tick();
    assert_eq!(retried.unsettled, None);
    assert!(host.store().waits().unwrap().is_empty(), "the next tick settles it");
}

#[test]
fn a_tick_caches_the_fleet_table_on_each_far_machine_it_pulled_so_its_clis_can_name_any_session() {
    let host = host("nexus");
    let _frames = live(&host, "worker");
    let far = Far::default();
    far.down.lock().unwrap().push("tom@bunker".into());
    let pulls = Pulls { spools: &far, always: vec![far_host("gex45"), far_host("bunker")] };

    let ticked = tick(&host.fleet, &mut host.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(far.cached.lock().unwrap().clone(), BTreeMap::from([("tom@gex45".to_owned(), vec!["worker".to_owned()])]));
    assert!(ticked.uncached.is_empty());
}

#[test]
fn a_session_gone_from_this_hosts_registry_loses_its_row_in_the_fleet_table() {
    let host = host("nexus");
    let _frames = live(&host, "worker");
    host.store().publish("nexus", &[SessionRow { name: "forgotten".into(), host: "nexus".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }]).unwrap();
    host.store().publish("gex45", &[SessionRow { name: "far".into(), host: "gex45".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }]).unwrap();

    host.tick();

    let statuses: Vec<(String, String)> = host.store().sessions().unwrap().into_iter().map(|row| (row.name, row.status)).collect();
    assert_eq!(statuses, [("far".to_owned(), "idle".to_owned()), ("worker".to_owned(), "idle".to_owned())]);
}

#[test]
fn a_job_exit_row_from_another_host_wakes_its_session_once_by_row_id() {
    let host = host("nexus");
    let frames = live(&host, "worker");
    let exit = FleetRow { kind: RowKind::JobExit, from: "gex45".into(), sender: None, text: "job soak exited 0".into(), job: Some(JobPayload { key: "soak".into(), lease_secs: None, exit: Some(0) }), ..message("x-1", "worker", NOW_MS) };
    let beat = FleetRow { id: "b-1".into(), kind: RowKind::JobBeat, job: Some(JobPayload { key: "soak".into(), lease_secs: Some(180), exit: None }), ..exit.clone() };
    host.store().put(&[beat, exit.clone()]).unwrap();

    let first = host.tick();
    let second = host.tick();

    assert_eq!(first.delivered, [("x-1".to_owned(), "worker".to_owned())]);
    assert_eq!((second.delivered.len(), second.already), (0, 0));
    assert_eq!(frame_uuids(&frames), [exit.prompt_id()]);
    assert!(frames.lock().unwrap()[0].contains("[from gex45] job soak exited 0"), "{:?}", frames.lock().unwrap());
}

struct Legacy<'a>(&'a Host);

impl FarSpools for Legacy<'_> {
    fn take(&self, _: &str) -> NorthResult<Pull> {
        Ok(Pull::default())
    }

    fn drop(&self, _: &str, _: &[String]) -> NorthResult<()> {
        Ok(())
    }

    fn set_aside(&self, _: &str, _: &[String]) -> NorthResult<()> {
        Ok(())
    }

    fn cache(&self, _: &str, _: &FleetTable) -> NorthResult<()> {
        Ok(())
    }

    fn registry(&self, _: &str) -> NorthResult<FarRegistry> {
        registry(&self.0.fleet, &self.0.fleet.sessions()?, NOW_MS)
    }

    fn deliver(&self, _: &str, rows: &[FleetRow]) -> NorthResult<Vec<HandedRow>> {
        Ok(hand_in_all(&self.0.fleet, rows, Instant::now(), BUDGET))
    }
}

#[test]
fn a_row_for_a_session_in_a_far_machines_own_registry_is_handed_in_there_once_by_the_tick_that_pulls_it() {
    let (nexus, gex45) = (host("nexus"), host("gex45"));
    let frames = live(&gex45, "perf");
    let row = message("m-legacy", "perf", NOW_MS);
    nexus.store().put(std::slice::from_ref(&row)).unwrap();
    let legacy = Legacy(&gex45);
    let pulls = Pulls { spools: &legacy, always: vec![far_host("gex45")] };

    let first = tick(&nexus.fleet, &mut nexus.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();
    let second = tick(&nexus.fleet, &mut nexus.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    assert_eq!(first.delivered, [("m-legacy".to_owned(), "perf".to_owned())], "{first:?}");
    assert_eq!((second.delivered.len(), second.already, second.failed.len()), (0, 0, 0), "{second:?}");
    assert_eq!(frame_uuids(&frames), [row.prompt_id()]);
    assert_eq!(hand_in_all(&gex45.fleet, std::slice::from_ref(&row), Instant::now(), BUDGET)[0].handed, crate::fleet::store::Handed::Already, "a second puller handing the row in again is a no-op on the far machine");
    let hosts: Vec<(String, String)> = nexus.store().sessions().unwrap().into_iter().map(|session| (session.name, session.host)).collect();
    assert_eq!(hosts, [("perf".to_owned(), "gex45".to_owned())]);
}

#[test]
fn a_far_machine_whose_own_registry_emptied_has_its_last_rows_removed() {
    let (nexus, gex45) = (host("nexus"), host("gex45"));
    nexus.store().publish("gex45", &[SessionRow { name: "forgotten".into(), host: "gex45".into(), status: "parked".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }]).unwrap();
    let legacy = Legacy(&gex45);
    let pulls = Pulls { spools: &legacy, always: vec![far_host("gex45")] };

    tick(&nexus.fleet, &mut nexus.store(), &pulls, &NoProbes, NOW_MS, BUDGET).unwrap();

    let statuses: Vec<(String, String)> = nexus.store().sessions().unwrap().into_iter().map(|row| (row.name, row.status)).collect();
    assert!(statuses.is_empty(), "{statuses:?}");
}

#[test]
fn a_forgotten_session_drops_out_of_its_hosts_cached_table_so_a_second_forget_is_refused() {
    let nexus = host("nexus");
    parked(&nexus, "worker", true);
    nexus.fleet.cache_sessions(&[SessionRow { name: "worker".into(), host: "nexus".into(), status: "exited".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }, SessionRow { name: "other".into(), host: "gex45".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }]).unwrap();

    assert!(matches!(nexus.fleet.control(ControlVerb::Forget, "worker").unwrap(), Controlled::Here));

    assert!(matches!(nexus.fleet.control(ControlVerb::Forget, "worker"), Err(NorthError::Usage(why)) if why == "no session named worker"));
    let kept: Vec<String> = nexus.fleet.cached_sessions().unwrap().unwrap().into_iter().map(|row| row.name).collect();
    assert_eq!(kept, ["other"]);
}

#[test]
fn a_tick_publishes_this_hosts_open_gates_into_the_store_and_caches_the_table_a_closed_gate_leaves_on_the_next_tick() {
    let nexus = host("nexus");
    let _worker = live(&nexus, "worker");
    let request = |question: &str, verification: bool| GateRequest { verification, issue: Some("north#416".into()), session: "worker".into(), question: question.into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 600_000 };
    let kept = nexus.fleet.open_gate(&request("Rent the box?", false)).unwrap();
    let closing = nexus.fleet.open_gate(&request("Buy the disk?", false)).unwrap();

    nexus.tick();

    let published: Vec<(String, i64)> = nexus.store().gates().unwrap().into_iter().map(|gate| (gate.host, gate.id)).collect();
    assert_eq!(published, [("nexus".to_owned(), kept), ("nexus".to_owned(), closing)]);
    let cached = crate::fleet::store::cached_gates(&nexus.root()).unwrap();
    assert_eq!(cached.iter().map(|gate| gate.label()).collect::<Vec<_>>(), [format!("nexus:{kept}"), format!("nexus:{closing}")]);

    nexus.fleet.answer_gate(closing, 1, None, None).unwrap();
    nexus.tick();

    let remaining: Vec<i64> = nexus.store().gates().unwrap().into_iter().map(|gate| gate.id).collect();
    assert_eq!(remaining, [kept]);
    assert_eq!(crate::fleet::store::cached_gates(&nexus.root()).unwrap().len(), 1);
}

#[test]
fn a_gate_held_on_another_host_reaches_the_laptop_through_the_store_and_its_issue_guard_and_is_answerable_by_its_qualified_id() {
    let (nexus, laptop) = (host("nexus"), host("laptop"));
    let _worker = live(&nexus, "worker");
    let request = GateRequest { verification: false, issue: Some("north#416".into()), session: "worker".into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 600_000 };
    let id = nexus.fleet.open_gate(&request).unwrap();
    nexus.tick();

    tick(&laptop.fleet, &mut nexus.store(), &laptop.pulls(), &NoProbes, NOW_MS, BUDGET).unwrap();

    let elsewhere = laptop.fleet.gates_elsewhere().unwrap();
    assert_eq!(elsewhere.iter().map(|gate| gate.label()).collect::<Vec<_>>(), [format!("nexus:{id}")]);
    assert_eq!(laptop.fleet.gated_issues().unwrap().get("north#416"), Some(&format!("nexus:{id}")));
    assert!(nexus.fleet.gates_elsewhere().unwrap().is_empty(), "a host lists its own gates from its ledger, not from the cache");
    laptop.fleet.answer_remote_gate(&format!("nexus:{id}"), None, 2, None, None).unwrap();
    let spooled = crate::fleet::store::Spool::at(&laptop.root()).pending().unwrap();
    assert_eq!(spooled.iter().map(|row| (row.kind, row.to.as_str(), row.answer)).collect::<Vec<_>>(), [(RowKind::GateAnswer, "worker", Some(AnswerPayload { gate: id, number: 2 }))]);
}

#[test]
fn a_cached_gate_whose_host_stopped_ticking_a_day_past_its_default_no_longer_holds_its_issue() {
    let laptop = host("laptop");
    let request = GateRequest { verification: false, issue: Some("north#416".into()), session: "worker".into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 600_000 };
    let at = |opened_ms: u64| crate::fleet::store::GateRow { host: "nexus".into(), id: 4, request: request.clone(), opened_ms };
    let now = crate::fleet::runtime::now_ms();
    let table = |opened_ms: u64| FleetTable { sessions: Vec::new(), gates: vec![at(opened_ms)] };

    laptop.fleet.cache_table(&table(now - 3_600_000)).unwrap();
    assert_eq!(laptop.fleet.gated_issues().unwrap().get("north#416"), Some(&"nexus:4".to_owned()));

    laptop.fleet.cache_table(&table(now - 2 * 24 * 3_600_000)).unwrap();
    assert!(laptop.fleet.gated_issues().unwrap().is_empty());
    assert!(laptop.fleet.gates_elsewhere().unwrap().is_empty());
}
