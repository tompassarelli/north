use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::Connection;

use super::attention::{Bug, Correction, Messages, Speaker, Typed, detector, messages, references};
use super::read::{alerts, bugs, corrections, landed, staffed};
use super::replay_tests::{NOW, at, judged, ledger};
use super::{Outcome, WEEK_S, cohorts, join, lines, weeks};
use crate::router::pricing::PriceTable;

const SONNET: f64 = 5.2;
const OPUS: f64 = 10.4;

fn approx(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-6
}

fn outcomes() -> Vec<Outcome> {
    let db = ledger();
    join(&staffed(&db, &PriceTable::load().unwrap()).unwrap(), &landed(&db).unwrap(), &judged(&db))
}

#[test]
fn each_staffed_run_joins_its_landed_commit_the_landers_first_commit_its_rework_and_its_priced_cost() {
    let found = outcomes();
    let row = |agent: &str| found.iter().find(|outcome| outcome.agent == agent).unwrap();
    let landing = |agent: &str| row(agent).landing.clone().map(|landing| (landing.sha[..2].to_owned(), landing.at, landing.first_commit, landing.reworked));
    assert_eq!(landing("s1"), Some(("a1".into(), at("2026-11-01T10:30:00Z"), Some(at("2026-11-01T09:30:00Z")), Some(true))), "fixed forward within 48 h");
    assert_eq!(landing("s2"), Some(("b2".into(), at("2026-11-01T12:30:00Z"), Some(at("2026-11-01T11:50:00Z")), Some(true))), "reverted");
    assert_eq!(landing("o1"), Some(("c3".into(), at("2026-11-01T14:30:00Z"), Some(at("2026-11-01T13:30:00Z")), Some(false))), "the failed landings row does not shadow the landed one");
    assert_eq!(landing("o2"), Some(("d4".into(), at("2026-11-03T14:20:00Z"), Some(at("2026-11-03T13:10:00Z")), Some(false))), "main's commit time, and runs.first_commit without a landings row");
    assert_eq!(landing("s3"), Some(("e5".into(), at("2026-11-02T09:30:00Z"), None, None)), "on no main, so unjudged");
    assert_eq!(landing("u1"), None);
    assert_eq!((row("s1").tier.as_str(), row("s1").arm.as_str(), row("o1").tier.as_str(), row("o1").arm.as_str(), row("u1").arm.as_str()), ("sonnet", "sonnet-high", "opus-medium", "worker", "unarmed"));
    assert!(approx(row("s1").usd, SONNET) && approx(row("o1").usd, OPUS), "{} {}", row("s1").usd, row("o1").usd);
}

#[test]
fn a_week_counts_landings_rework_cost_per_surviving_landing_and_first_commit_to_main_per_tier_and_arm() {
    let (total, by) = cohorts(&outcomes(), NOW - WEEK_S, NOW, NOW);
    assert_eq!((total.runs, total.landed, total.reworked, total.unjudged, total.maturing, total.surviving()), (6, 5, 2, 1, 1, 3));
    assert!(approx(total.usd, 3.0 * SONNET + 3.0 * OPUS));
    assert!(approx(total.usd_per_surviving().unwrap(), (3.0 * SONNET + 3.0 * OPUS) / 3.0));
    assert!(approx(total.rework_rate().unwrap(), 0.4));
    assert_eq!((total.first_commit_p50(), total.first_commit_minutes.len()), (Some(60.0), 4));
    let keys: Vec<(&str, &str, usize, usize, usize)> = by.iter().map(|cohort| (cohort.tier.as_str(), cohort.arm.as_str(), cohort.runs, cohort.landed, cohort.reworked)).collect();
    assert_eq!(keys, [("opus-medium", "unarmed", 1, 0, 0), ("opus-medium", "worker", 2, 2, 0), ("sonnet", "sonnet-high", 3, 3, 2)]);
    let sonnet = &by[2];
    assert!(approx(sonnet.usd_per_surviving().unwrap(), 3.0 * SONNET), "one surviving Sonnet landing carries all three runs");
    assert_eq!(by[1].first_commit_p50(), Some(65.0));
    let (earlier, _) = cohorts(&outcomes(), NOW - 2 * WEEK_S, NOW - WEEK_S, NOW);
    assert_eq!((earlier.runs, earlier.landed, earlier.reworked, earlier.maturing), (1, 1, 0, 0));
}

fn typed(id: i64, session: &str, offset: i64, text: &str) -> Typed {
    Typed { id, session: session.into(), at: at("2026-11-02T10:00:00Z") + offset, text: text.into() }
}

fn tom() -> Vec<Typed> {
    vec![typed(1, "tom-1", 0, "please make the lander faster, see tompassarelli/north#13"), typed(3, "tom-1", 1800, "and file it")]
}

fn delivered() -> Vec<(String, i64, String)> {
    let t0 = at("2026-11-02T10:00:00Z");
    let row = |session: &str, offset: i64, text: &str| (session.to_owned(), t0 + offset, text.to_owned());
    vec![
        row("tom-1", 600, "[from gex45] watchdog: host overload while north#15 ran"),
        row("w-1", -7200, "Item: north#14 the queue Category: feature"),
        row("w-1", -3600, "[urgent message] [from gex45] watchdog: north#14 stalled"),
        row("w-1", -3000, "north land: n14 did not land"),
        row("w-1", -2000, "[from lead] Lead: north#13 is yours"),
    ]
}

#[test]
fn tom_messages_are_his_typed_turns_and_detectors_are_alerts_delivered_into_any_conversation() {
    let found = messages(&tom(), &delivered());
    let speakers: Vec<(&str, Speaker)> = found.0.iter().map(|message| (message.session.as_str(), message.speaker)).collect();
    assert_eq!(speakers, [("w-1", Speaker::Detector), ("w-1", Speaker::Detector), ("tom-1", Speaker::Tom), ("tom-1", Speaker::Detector), ("tom-1", Speaker::Tom)]);
    assert!(detector("[urgent message] [from gex45] watchdog: x") && !detector("[from lead] Lead: watchdog: x"));
    assert_eq!(references("see tompassarelli/North#13 and wisp#2, not #4").into_iter().collect::<Vec<_>>(), ["north#13", "wisp#2"]);
    let typed = BTreeSet::from([1]);
    assert_eq!(Correction::from_row(0, "direction", "tom convo:2 session:s", "build only here", &typed), None, "a message that is not one of Tom's turns is not his correction");
    assert_eq!(Correction::from_row(0, "decision", "tom convo:1 session:s", "yes", &typed), None);
    assert_eq!(Correction::from_row(0, "correction", "proxy convo:1", "no", &typed), None);
    assert!(Correction::from_row(0, "correction", "tom convo:1 session:s", "no, use the queue", &typed).is_some());
}

#[test]
fn attention_spans_each_conversation_and_incidents_go_to_whoever_named_them_first() {
    let found = messages(&tom(), &delivered());
    let t0 = at("2026-11-02T10:00:00Z");
    let typed = BTreeSet::from([1, 3]);
    let corrections = [Correction::from_row(t0 + 900, "correction", "tom convo:1", "no, queue north#14 instead", &typed).unwrap(), Correction::from_row(t0 + 1200, "direction", "tom convo:3", "stop soaking", &typed).unwrap()];
    let bugs = [Bug { item: "tompassarelli/north#15".into(), at: at("2026-11-02T08:00:00Z") }, Bug { item: "north#99".into(), at: t0 }];
    let attention = found.attention(&corrections, NOW - WEEK_S, NOW);
    assert_eq!((attention.messages, attention.conversations, attention.active_days, attention.corrections), (2, 1, 1, 2));
    assert!(approx(attention.minutes, 30.0));
    assert!(attention.line().contains("corrections per active day 2.0 (2/1)"), "{}", attention.line());
    let incidents = found.incidents(&corrections, &bugs, NOW - WEEK_S, NOW);
    assert_eq!((incidents.corrections, incidents.bugs, incidents.tom, incidents.detector, incidents.neither), (2, 2, 1, 2, 1));
    assert_eq!(incidents.line(), "incidents 4 (2 corrections, 2 filed bugs): first noticed by Tom 1/4, by a detector 2/4, named by neither 1/4");
    assert_eq!(Messages::default().attention(&[], 0, NOW).line(), "Tom's attention 0 messages over 0 conversations, 0.0 h from first to last message per conversation, 0 active days; corrections per active day n/a (0/0)");
}

#[test]
fn north_flow_prints_both_weeks_with_every_denominator() {
    let db = ledger();
    let found = messages(&tom(), &delivered());
    let printed = lines(&weeks(&outcomes(), &found, &[], &bugs(&db).unwrap(), NOW), "the fixture").join("\n");
    let expected = "outcome 7d [2026-10-28T12:00:00Z, 2026-11-04T12:00:00Z): landed 5 (reworked 2/5 = 40%, 1 not on any main, 1 under 48 h), cost per surviving landing $15.60 ($46.80 over 6 runs / 3 surviving), first-commit-to-main p50 60m (n=4/5), landed per hour of Tom's attention 10.00 (5/0.5 h)";
    assert!(printed.starts_with("outcome sources: the fixture\n"), "{printed}");
    assert!(printed.contains(expected), "{printed}");
    assert!(printed.contains("outcome 7d [2026-10-28T12:00:00Z, 2026-11-04T12:00:00Z) sonnet arm sonnet-high: landed 3 (reworked 2/3 = 67%"), "{printed}");
    assert!(printed.contains("incidents 1 (0 corrections, 1 filed bugs): first noticed by Tom 0/1, by a detector 1/1"), "{printed}");
    assert!(printed.contains("outcome 7d [2026-10-21T12:00:00Z, 2026-10-28T12:00:00Z): landed 1 (reworked 0/1 = 0%"), "{printed}");
    assert!(printed.contains("landed per hour of Tom's attention n/a (1/0.0 h)"), "{printed}");
}

fn convo_store(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT, session_id TEXT, agent TEXT, last_ts TEXT);
         CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INTEGER, line INTEGER, ts TEXT, role TEXT, tool TEXT, body INTEGER, dup INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT);
         INSERT INTO files VALUES (1, '/t/a.jsonl', 'tom-1', NULL, '2026-11-02T11:00:00Z'), (2, '/t/b.jsonl', 'sub', 'agent-1', '2026-11-02T11:00:00Z'), (3, '/t/c.jsonl', 'gone', NULL, '2026-09-01T00:00:00Z');
         INSERT INTO body VALUES (1, '[from gex45] watchdog: north#13 stalled'), (2, 'and now'), (3, 'tool output'), (4, 'old words'), (5, 'more old words');
         INSERT INTO msg(file_id, line, ts, role, tool, body, dup) VALUES (1, 1, '2026-11-02T10:00:00.000Z', 'user', NULL, 1, 0), (1, 2, '2026-11-02T10:20:00.000Z', 'user', NULL, 2, 0),
           (1, 3, '2026-11-02T10:21:00.000Z', 'user', 'Bash', 3, 0), (1, 4, '2026-11-02T10:22:00.000Z', 'user', NULL, 2, 1), (2, 1, '2026-11-02T10:00:00.000Z', 'user', NULL, 1, 0),
           (3, 1, '2026-09-01T00:00:00.000Z', 'user', NULL, 4, 0), (3, 2, '2026-09-01T00:01:00.000Z', 'user', NULL, 5, 0);",
    )
    .unwrap();
}

#[test]
fn the_convo_store_reader_keeps_the_detector_alerts_of_main_conversations_active_in_the_window() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index-v4.db");
    convo_store(&path);
    let found = alerts(&path, NOW - 3 * WEEK_S).unwrap();
    assert_eq!(found, [("tom-1".to_owned(), at("2026-11-02T10:00:00Z"), "[from gex45] watchdog: north#13 stalled".to_owned())], "an agent's file, a tool result, a duplicate, an old conversation and a plain message are not alerts");
    assert!(alerts(&directory.path().join("missing.db"), 0).unwrap().is_empty());
}

#[test]
fn the_grounding_ledger_reader_keeps_the_corrections_and_directions_whose_message_is_one_of_toms_turns() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.jsonl");
    let rows = [
        r#"{"id":"1","at":"2026-11-02T10:00:00Z","kind":"correction","topic":"t","quote":"no, north#4","source":"tom convo:1 session:s","superseded_by":null}"#,
        r#"{"id":"2","at":"2026-11-02T10:00:00Z","kind":"direction","topic":"t","quote":"do it","source":"tom convo:2 session:s","superseded_by":null}"#,
        r#"{"id":"3","at":"2026-11-02T10:00:00Z","kind":"decision","topic":"t","quote":"DECIDED x","source":"proxy convo:3","superseded_by":null}"#,
        "not json",
    ];
    std::fs::write(&path, rows.join("\n")).unwrap();
    let found = corrections(&path, &[typed(1, "s", 0, "no, north#4")]);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].at, found[0].names.iter().cloned().collect::<Vec<_>>()), (at("2026-11-02T10:00:00Z"), vec!["north#4".to_owned()]));
    assert!(corrections(&directory.path().join("missing.jsonl"), &tom()).is_empty());
}
