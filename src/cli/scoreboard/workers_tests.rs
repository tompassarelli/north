use std::collections::BTreeSet;
use std::fs;

use rusqlite::Connection;

use crate::land_ticket::lanes_by_session;

use super::{Pending, attempts};
use crate::platform::land_queue;
use crate::threads::attribution::Cause;

/// n-takeable queued its north#706 lane and reported `Stop: landing` at 10:10 AM on 2026-11-07, and the ready bar counted
/// that first try lost to the harness while the lander still held its ticket (north#751): a run whose session has an
/// undecided ticket is pending, the same report whose ticket was refused counts as a landing refusal, and one whose ticket landed is done.
#[test]
fn a_first_try_whose_reviewer_lane_landed_it_is_not_pending_and_not_lost() {
    let state = tempfile::tempdir().unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    fs::create_dir_all(queue.join("done")).unwrap();
    fs::write(queue.join("1794020829623648-n706-claim-filed.json"), r#"{"session":"n-takeable"}"#).unwrap();
    fs::write(queue.join("done/1794020468167698-n706-boxneeds.json"), r#"{"session":"n-decided"}"#).unwrap();
    fs::write(queue.join("done/1794018600000000-ingest-chunks.json"), r#"{"session":"n-landed","verdict":"landed ba47d1dd7ee370ecebbf378f4b77260e337e036a"}"#).unwrap();
    fs::write(queue.join("1794020829623649-n706-reviewing.json"), r#"{"session":"n-handed-rv"}"#).unwrap();
    fs::write(queue.join("done/1794018600000001-n706-reviewed.json"), r#"{"session":"n-reviewed-rv","verdict":"landed ba47d1dd7ee370ecebbf378f4b77260e337e036a"}"#).unwrap();
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT);").unwrap();
    for (agent, session, report) in [("queued", "n-takeable", "Not done: the lane is queued."), ("decided", "n-decided", "Not done: the lane is queued.\nStop: landing"), ("landed", "n-landed", "Not done: the lane is queued.\nStop: landing"), ("handed", "n-handed", "Not done: handed to the reviewer.\nStop: landing"), ("reviewed", "n-reviewed", "Not done: handed to the reviewer.\nStop: landing")] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#706', NULL, 'bug', '2026-11-07T02:21:27Z', '2026-11-07T03:10:21Z', 49, 'not done', ?3, 'worker', '', 0, ?2)", [agent, session, report]).unwrap();
    }
    let read = attempts(&db, 0, &BTreeSet::new(), &Pending { lanes: lanes_by_session(&land_queue::queues(state.path())).unwrap(), waking: BTreeSet::new(), ..Pending::default() }).unwrap();
    let causes: Vec<(Cause, bool)> = read.iter().map(|attempt| (attempt.cause, attempt.succeeded)).collect();
    assert_eq!(causes, [(Cause::Worker, false), (Cause::Worker, true), (Cause::Worker, true)], "a worker whose '-rv' reviewer session landed its lane is done and one whose reviewer still holds the ticket is pending; a decided lane no lander record names is the worker's whatever its Stop: line says (north#872); north-255-d1828 queued a lane that landed six minutes after its Stop: landing report on 2026-11-07, and read as lost until the ledger's join caught up");
}



