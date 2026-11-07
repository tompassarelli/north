use std::collections::BTreeSet;
use std::fs;

use rusqlite::Connection;

use super::attempts;
use crate::platform::land_queue;
use crate::threads::attribution::{Cause, Harness};

/// n-takeable queued its north#706 lane and reported `Stop: landing` at 10:10 AM on 2026-11-07, and the ready bar counted
/// that first try lost to the harness while the lander still held its ticket (north#751): a run whose session has an
/// undecided ticket is pending, and the same report whose ticket is decided counts as a landing refusal.
#[test]
fn a_landing_stop_is_pending_while_its_ticket_waits_in_the_lander_queue() {
    let state = tempfile::tempdir().unwrap();
    let queue = state.path().join("north/land-queue/de2e0e8479546ba9");
    fs::create_dir_all(queue.join("done")).unwrap();
    fs::write(queue.join("1794020829623648-n706-claim-filed.json"), r#"{"session":"n-takeable"}"#).unwrap();
    fs::write(queue.join("done/1794020468167698-n706-boxneeds.json"), r#"{"session":"n-decided"}"#).unwrap();
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT);").unwrap();
    for (agent, session) in [("queued", "n-takeable"), ("decided", "n-decided")] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#706', NULL, 'bug', '2026-11-07T02:21:27Z', '2026-11-07T03:10:21Z', 49, 'not done', 'Not done: the lane is queued.\nStop: landing', 'worker', '', 0, ?2)", [agent, session]).unwrap();
    }
    let read = attempts(&db, 0, &BTreeSet::new(), &land_queue::queued_sessions(state.path())).unwrap();
    let causes: Vec<Cause> = read.iter().map(|attempt| attempt.cause).collect();
    assert_eq!(causes, [Cause::Harness(Harness::LandingRefusal)]);
}

/// north-751-d0501 ended `Stop: wait until 2026-11-07T05:45Z` on 2026-11-07 because its only open box measured a time
/// window: first-try success leaves such a deferred box out, but goals.toml counts waits among harness losses.
#[test]
fn a_first_try_deferred_to_a_dated_check_is_left_out_of_first_try_and_counts_as_a_waits_loss() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT);").unwrap();
    db.execute("INSERT INTO runs VALUES ('d0501', 'north#751', NULL, 'bug', '2026-11-07T05:01:00Z', '2026-11-07T05:10:00Z', 9, 'not done', 'Not done: the 1 h cell cannot pass before 12:45 PM.\nStop: wait until 2026-11-07T05:45Z', 'worker', '', 0, 'north-751-d0501')", []).unwrap();
    let read = attempts(&db, 0, &BTreeSet::new(), &BTreeSet::new()).unwrap();
    assert_eq!(read.iter().map(|attempt| (attempt.deferred, attempt.cause)).collect::<Vec<_>>(), [(true, Cause::Harness(Harness::Waits))]);
}
