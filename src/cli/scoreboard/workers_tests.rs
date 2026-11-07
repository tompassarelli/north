use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use rusqlite::Connection;

use crate::land_ticket::lanes_by_session;

use super::{Pending, attempts, waking_sessions};
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
    for (agent, session, report) in [("queued", "n-takeable", "Not done: the lane is queued."), ("decided", "n-decided", "Not done: the lane is queued.\nStop: landing")] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#706', NULL, 'bug', '2026-11-07T02:21:27Z', '2026-11-07T03:10:21Z', 49, 'not done', ?3, 'worker', '', 0, ?2)", [agent, session, report]).unwrap();
    }
    let read = attempts(&db, 0, &BTreeSet::new(), &Pending { lanes: lanes_by_session(&land_queue::queues(state.path())).unwrap(), waking: BTreeSet::new(), ..Pending::default() }).unwrap();
    let causes: Vec<Cause> = read.iter().map(|attempt| attempt.cause).collect();
    assert_eq!(causes, [Cause::Harness(Harness::LandingRefusal)]);
}

/// north-751-d0501 ended `Stop: wait until 2026-11-07T05:45Z` on 2026-11-07 because its only open box measured a time
/// window: first-try success leaves a box deferred through `north check experiments defer` out and counts a waits loss
/// (north#791), where an identical report from a session that recorded no deferral is the worker's.
#[test]
fn a_first_try_deferred_by_a_recorded_deferral_is_left_out_of_first_try_and_counts_as_a_waits_loss_and_the_same_words_alone_do_not() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT); CREATE TABLE boundary_causes(session TEXT NOT NULL, source TEXT NOT NULL, cause TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', at TEXT NOT NULL);").unwrap();
    for (agent, session) in [("d0501", "north-751-d0501"), ("d0502", "north-751-d0502")] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#751', NULL, 'bug', '2026-11-07T05:01:00Z', '2026-11-07T05:10:00Z', 9, 'not done', 'Not done: the 1 h cell cannot pass before 12:45 PM.\nStop: wait until 2026-11-07T05:45Z', 'worker', '', 0, ?2)", [agent, session]).unwrap();
    }
    db.execute("INSERT INTO boundary_causes(session, source, cause, at) VALUES ('north-751-d0501', 'deferral', 'waits', '2026-11-07T05:08:00Z')", []).unwrap();
    let read = attempts(&db, 0, &BTreeSet::new(), &Pending::default()).unwrap();
    assert_eq!(read.iter().map(|attempt| (attempt.deferred, attempt.cause)).collect::<Vec<_>>(), [(true, Cause::Harness(Harness::Waits)), (false, Cause::Worker)]);
}

/// north-730-d0351 ended `Stop: wait until 2026-11-07T06:30Z` at 5:41 AM UTC on 2026-11-07 while its `job-run --notify`
/// jobs still held their leases in fleet_waits, and the ready bar counted that first try lost to the harness although
/// the jobs' exits wake the session to report again (north#751); once the lease runs out the same report counts.
#[test]
fn an_unfinished_first_try_is_pending_while_its_notify_job_holds_its_lease() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT); CREATE TABLE fleet_waits(session TEXT, kind TEXT, key TEXT, deadline TEXT, created TEXT);").unwrap();
    db.execute("INSERT INTO runs VALUES ('d0351', 'north#730', NULL, 'bug', '2026-11-07T03:52:00Z', '2026-11-07T05:41:00Z', 109, 'not done', 'Not done: the fix is queued; the job will message me.\nStop: wait until 2026-11-07T06:30Z', 'worker', '', 0, 'north-730-d0351')", []).unwrap();
    db.execute("INSERT INTO fleet_waits VALUES ('north-730-d0351', 'job', 'n730-poolkey-landed@gex45#2758963', '2026-11-07T06:03:39Z', '2026-11-07T05:41:48Z')", []).unwrap();
    let at = |iso: &str| crate::foundation::clock::Timestamp::parse(iso).unwrap().unix_seconds();
    let read = |now: &str| attempts(&db, 0, &BTreeSet::new(), &Pending { lanes: Default::default(), waking: waking_sessions(&db, at(now)), ..Pending::default() }).unwrap().len();
    assert_eq!((read("2026-11-07T06:00:00Z"), read("2026-11-07T06:05:00Z")), (0, 1));
}

/// On 2026-11-07 28 of the 139 Done runs over 24 h that expected a landing had no landing on their row, and the first-try
/// bar counted them successes because it read the `Done:` opener alone (north#791): done is a landed sha, not a word.
#[test]
fn a_done_run_that_expected_a_landing_and_has_no_landed_sha_is_not_a_first_try_success() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT, expects_landing INTEGER, landed INTEGER, landed_sha TEXT);").unwrap();
    for (agent, expects, landed, sha) in [("landed", 1, 1, Some("abc1234")), ("unlanded", 1, 0, None), ("no-landing-expected", 0, 0, None)] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#791', NULL, 'bug', '2026-11-07T02:21:27Z', '2026-11-07T03:10:21Z', 49, 'done', 'Done: finished.', 'worker', '', 0, 's', ?2, ?3, ?4)", rusqlite::params![agent, expects, landed, sha]).unwrap();
    }
    let read = attempts(&db, 0, &BTreeSet::new(), &Pending::default()).unwrap();
    assert_eq!(read.iter().map(|attempt| attempt.succeeded).collect::<Vec<_>>(), [true, false, true]);
}



fn chain_db(causes: &[(&str, &str, &str)], runs: &[(&str, &str, &str, &str)]) -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT); CREATE TABLE boundary_causes(session TEXT NOT NULL, source TEXT NOT NULL, cause TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', at TEXT NOT NULL);").unwrap();
    for (agent, started, ended, report) in runs {
        let outcome = if report.starts_with("Done") { "done" } else { "not done" };
        db.execute("INSERT INTO runs VALUES (?1, 'north#722', NULL, 'bug', ?2, ?3, 30, ?4, ?5, 'worker', '', 0, 'n-dead-cites')", [agent, started, ended, outcome, report]).unwrap();
    }
    for (source, cause, at) in causes {
        db.execute("INSERT INTO boundary_causes(session, source, cause, at) VALUES ('n-dead-cites', ?1, ?2, ?3)", [source, cause, at]).unwrap();
    }
    db
}

/// n-dead-cites stopped on `Stop: landing` at 9:00 AM on 2026-11-07 and the harness resumed it when the lander decided: the
/// first try is judged by the run that landed (north#790), where a resume the proxy or Tom followed by a message of their own
/// leaves the stopped run a lost first try.
#[test]
fn a_run_the_harness_resumed_is_judged_by_where_its_chain_ends_and_a_correction_in_between_keeps_it_lost() {
    let landing = "Not done: the lane is queued.\nStop: landing";
    let runs = [("a", "2026-11-07T15:00:00Z", "2026-11-07T16:00:00Z", landing), ("b", "2026-11-07T16:30:00Z", "2026-11-07T17:00:00Z", "Done: landed.")];
    let resumed = chain_db(&[("resume", "harness:landing-refusal", "2026-11-07T16:20:00Z")], &runs);
    let read = attempts(&resumed, 0, &BTreeSet::new(), &Pending::default()).unwrap();
    assert_eq!(read.iter().map(|attempt| attempt.succeeded).collect::<Vec<_>>(), [true]);
    let corrected = chain_db(&[("resume", "harness:landing-refusal", "2026-11-07T16:20:00Z"), ("proxy", "proxy", "2026-11-07T16:25:00Z")], &runs);
    let read = attempts(&corrected, 0, &BTreeSet::new(), &Pending::default()).unwrap();
    assert_eq!(read.iter().map(|attempt| attempt.succeeded).collect::<Vec<_>>(), [false, true]);
    let pending = chain_db(&[("resume", "harness:landing-refusal", "2026-11-07T16:20:00Z")], &runs[..1]);
    assert!(attempts(&pending, 0, &BTreeSet::new(), &Pending::default()).unwrap().is_empty());
}

/// Ten of the 66 runs that closed 2026-11-06 23:00 to 2026-11-07 06:14 UTC ended `Stop: capacity` with no boundary record,
/// and the bar took the worker's word for it (north#791): the capacity host's `capacity_priority` rows name the session
/// (owner `gate:SESSION`) and how long its run queued for admission, so a failed run whose session queued past the bound
/// inside its window is a capacity loss whatever its report says, and a session that queued briefly or outside it is not.
#[test]
fn a_failed_run_whose_session_queued_past_the_bound_for_capacity_is_a_capacity_loss_without_its_stop_line() {
    let capacity = Connection::open_in_memory().unwrap();
    capacity.execute_batch("CREATE TABLE capacity_priority (lease TEXT PRIMARY KEY, at TEXT NOT NULL, owner TEXT NOT NULL, class TEXT NOT NULL, priority TEXT NOT NULL, placed INTEGER NOT NULL, queuedSeconds REAL NOT NULL, command TEXT)").unwrap();
    for (lease, at, owner, queued) in [("a", "2026-11-07T04:30:00.000Z", "gate:north-757-d0001", 900.0), ("b", "2026-11-07T04:30:00.000Z", "gate:north-757-d0002", 20.0), ("c", "2026-11-07T01:00:00.000Z", "gate:north-757-d0003", 900.0)] {
        capacity.execute("INSERT INTO capacity_priority VALUES (?1, ?2, ?3, 'test', 'tom-ask', 1, ?4, NULL)", rusqlite::params![lease, at, owner, queued]).unwrap();
    }
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT);").unwrap();
    for (agent, session) in [("d0001", "north-757-d0001"), ("d0002", "north-757-d0002"), ("d0003", "north-757-d0003")] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#757', NULL, 'bug', '2026-11-07T04:00:00Z', '2026-11-07T05:00:00Z', 60, 'not done', 'Not done: the check is queued.', 'worker', '', 0, ?2)", [agent, session]).unwrap();
    }
    let read = attempts(&db, 0, &BTreeSet::new(), &Pending { capacity: Some(capacity), ..Pending::default() }).unwrap();
    assert_eq!(read.iter().map(|attempt| attempt.cause).collect::<Vec<_>>(), [Cause::Harness(Harness::Capacity), Cause::Worker, Cause::Worker]);
}

/// The ten `Stop: wait until` runs of 2026-11-06 23:00 to 2026-11-07 06:14 UTC had no row in `fleet_waits`, which drops a wait
/// at its end (north#791): `fleet_wait_history` keeps each settled wait. A wait on an unlanded branch lost the run to waits,
/// while a wait on the session's own job did not, since that job's end wakes the session to report again (north#706), and a
/// wait registered after the run's window is no evidence about it.
#[test]
fn only_a_registered_wait_on_something_the_session_did_not_run_is_a_waits_loss_after_it_settled() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT); CREATE TABLE fleet_wait_history(session TEXT, kind TEXT, key TEXT, created TEXT, settled TEXT);").unwrap();
    for (agent, kind, started, ended) in [("d0351", "job", "2026-11-07T03:52:00Z", "2026-11-07T05:41:00Z"), ("d0352", "branch", "2026-11-07T01:00:00Z", "2026-11-07T05:41:00Z"), ("d0353", "branch", "2026-11-07T01:00:00Z", "2026-11-07T02:00:00Z")] {
        db.execute("INSERT INTO runs VALUES (?1, 'north#730', NULL, 'bug', ?2, ?3, 60, 'not done', 'Not done: the fix is queued.', 'worker', '', 0, ?4)", rusqlite::params![agent, started, ended, format!("north-730-{agent}")]).unwrap();
        db.execute("INSERT INTO fleet_wait_history VALUES (?1, ?2, 'n730-poolkey-landed', '2026-11-07T05:41:48Z', '2026-11-07T06:03:39Z')", rusqlite::params![format!("north-730-{agent}"), kind]).unwrap();
    }
    let read = attempts(&db, 0, &BTreeSet::new(), &Pending::default()).unwrap();
    assert_eq!(read.iter().map(|attempt| attempt.cause).collect::<Vec<_>>(), [Cause::Harness(Harness::Waits), Cause::Worker, Cause::Worker]);
}

/// north-697-d0601 ended `Stop: capacity` on 2026-11-07 while its own cargo nextest waited in admission for a heavy lease
/// (north#706): the run stays open while the session's build is queued or running there, and counts once the build is gone.
#[test]
fn a_first_try_is_pending_while_the_sessions_own_build_waits_in_capacity_admission() {
    let admission = tempfile::tempdir().unwrap();
    let me = std::process::id();
    let stat = fs::read_to_string(format!("/proc/{me}/stat")).unwrap();
    let start = stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(19).unwrap().to_owned();
    fs::create_dir_all(admission.path().join("queue")).unwrap();
    fs::write(admission.path().join("queue/ticket.json"), format!(r#"{{"owner":"gate:north-697-d0601","wrapperPid":{me},"wrapperStart":"{start}"}}"#)).unwrap();
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT);").unwrap();
    db.execute("INSERT INTO runs VALUES ('d0601', 'north#697', NULL, 'bug', '2026-11-07T05:00:00Z', '2026-11-07T06:00:00Z', 60, 'not done', 'Not done: the test run is queued.\nStop: capacity', 'worker', '', 0, 'north-697-d0601')", []).unwrap();
    let read = |root: &Path| attempts(&db, 0, &BTreeSet::new(), &Pending { admission: Some(root.to_path_buf()), ..Pending::default() }).unwrap().len();
    assert_eq!((read(admission.path()), read(tempfile::tempdir().unwrap().path())), (0, 1));
}

/// north-750-d1226 landed its server half on gex45 and stopped for the box only the laptop can prove at 12:29 UTC on
/// 2026-11-07, and the bar counted that first try lost to placement before dispatch could relaunch it (north#751): the host
/// need the session recorded with `north fleet need` holds it for its relaunch for a day, whatever its report says
/// (north#791), and a `Stop: host` line with no need row is the worker's.
#[test]
fn a_first_try_whose_session_recorded_a_host_need_waits_on_its_relaunch_for_a_day_and_a_stop_line_alone_does_not() {
    let at = |iso: &str| crate::foundation::clock::Timestamp::parse(iso).unwrap().unix_seconds();
    let read = |db: &Connection, now: &str| attempts(db, 0, &BTreeSet::new(), &Pending { now: at(now), ..Pending::default() }).unwrap().len();
    let runs = |report: &'static str| [("a", "2026-11-07T12:00:00Z", "2026-11-07T12:29:00Z", report)];
    let recorded = chain_db(&[("host-need", "harness:placement", "2026-11-07T12:28:00Z")], &runs("Not done: this session has no route to the laptop."));
    assert_eq!((read(&recorded, "2026-11-07T12:47:00Z"), read(&recorded, "2026-11-08T12:30:00Z")), (0, 1));
    let said = chain_db(&[], &runs("Not done: this session has no route to the laptop.\nStop: host laptop"));
    assert_eq!(read(&said, "2026-11-07T12:47:00Z"), 1);
}
