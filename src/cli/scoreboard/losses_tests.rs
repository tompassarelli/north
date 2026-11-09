use std::collections::BTreeSet;

use rusqlite::Connection;

use super::{Request, render};
use crate::foundation::clock::Timestamp;
use crate::cli::scoreboard::workers;

fn at(iso: &str) -> i64 {
    Timestamp::parse(iso).unwrap().unix_seconds()
}

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, follows TEXT, category TEXT, started TEXT, ended TEXT, minutes INTEGER, outcome TEXT, report TEXT, role TEXT, brief_text TEXT, compactions INTEGER, fleet_name TEXT); CREATE TABLE boundary_causes(session TEXT NOT NULL, source TEXT NOT NULL, cause TEXT NOT NULL, reason TEXT NOT NULL DEFAULT '', at TEXT NOT NULL);").unwrap();
    db
}

fn run(db: &Connection, [agent, item, started, ended, outcome, report, session]: [&str; 7]) {
    db.execute("INSERT INTO runs VALUES (?1, ?2, NULL, 'bug', ?3, ?4, 60, ?5, ?6, 'worker', '', 0, ?7)", [agent, item, started, ended, outcome, report, session]).unwrap();
}

#[test]
fn the_losses_list_names_each_first_try_the_bar_charged_to_the_harness_with_its_class_and_boundary_record() {
    let db = db();
    run(&db, ["d0411", "north#725", "2026-11-07T03:12:00Z", "2026-11-07T04:12:00Z", "not done", "Not done: the box needs the laptop's row.", "north-725-d0411"]);
    db.execute("INSERT INTO boundary_causes(session, source, cause, reason, at) VALUES ('north-725-d0411', 'dispatch', 'placement', 'the box needs the laptop row', '2026-11-07T03:30:00Z')", []).unwrap();
    db.execute("INSERT INTO boundary_causes(session, source, cause, reason, at) VALUES ('north-725-d0411', 'resume', 'landing refusal', 'the lane was resumed', '2026-11-07T03:40:00Z')", []).unwrap();
    run(&db, ["d0501", "north#725", "2026-11-07T03:12:00Z", "2026-11-07T04:20:00Z", "not done", "Not done: the wrong approach.\nStop: reasoning", "north-725-d0501"]);
    run(&db, ["d0401", "north#725", "2026-11-07T03:52:00Z", "2026-11-07T04:15:00Z", "not done", "Not done: the build is queued behind capacity.\nStop: capacity", "north-725-d0401"]);
    run(&db, ["d0301", "north#725", "2026-11-07T04:20:00Z", "2026-11-07T04:30:00Z", "done", "Done: landed as abc1234.", "north-725-d0301"]);

    let attempts = workers::attempts(&db, 0, &BTreeSet::new(), &workers::Pending::default()).unwrap();
    let request = Request { since_s: 86_400, until: Some(at("2026-11-07T05:00:00Z")), repo: "north".to_owned() };
    let text = render(&db, &attempts, &request, at("2026-11-07T04:00:00Z"), at("2026-11-07T05:00:00Z"));

    assert_eq!(
        text,
        "north: 2 of 4 first tries lost to the harness in 24.0 h to 2026-11-07T05:00:00Z\nclasses: capacity 1, placement 1\n2026-11-07T04:12:00Z north-725-d0411 north#725 [placement] recorded by dispatch as placement at 2026-11-07T03:30:00Z: the box needs the laptop row\n2026-11-07T04:15:00Z north-725-d0401 north#725 [capacity] no boundary row; the class came from the run's final report\n"
    );
}
