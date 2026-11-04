use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use super::disk;
use super::plan::{self, Doc, IssueBox, Landed};
use super::refusal::{self, Refusal};
use super::signal::{self, Signal};
use super::{Detector, Filer, Finding, Limits};
use crate::foundation::clock::Timestamp;
use crate::ops::flow::{Host, Places};

const DOC: &str = include_str!("fixtures/docs-architecture.md");
const LANDED: &str = include_str!("fixtures/landed-docs-architecture.tsv");
const HEADER_TIMES: &str = include_str!("fixtures/usage-header-times.txt");
const BUSY_LOG: &str = include_str!("fixtures/land-check-text-file-busy.log");
const DOC_ADDED: f64 = 1_793_764_379.0;
const HOUR: f64 = 3600.0;

fn limits() -> Limits {
    Limits { stall_s: 2.0 * HOUR, silence_cadences: 3.0, refusal_window_s: 24.0 * HOUR, refusal_min: 2, disk_floor_gib: 15.0 }
}

fn at(text: &str) -> f64 {
    Timestamp::parse_python_datetime(text).expect("timestamp").unix_seconds_f64()
}

fn landed(until: f64) -> Vec<Landed> {
    LANDED.lines().filter_map(|line| line.split_once('\t')).map(|(at, subject)| Landed { at: at.parse().expect("epoch"), subject: subject.to_owned() }).filter(|commit| commit.at <= until).collect()
}

fn plan_facts(until: f64, names: &[&str]) -> plan::Facts {
    plan::Facts { docs: vec![Doc { slug: "docs-architecture".into(), added_at: DOC_ADDED, text: DOC.into() }], landed: landed(until), names: names.iter().map(|name| (*name).to_owned()).collect(), issues: Vec::new() }
}

fn stalled_ids(until: f64, now: f64, names: &[&str]) -> BTreeSet<String> {
    plan::stalled(&plan_facts(until, names), now, &limits()).into_iter().flat_map(|finding| finding.instances).map(|instance| instance.rsplit('#').next().unwrap_or_default().to_owned()).collect()
}

fn ids(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn the_plan_parser_reads_every_box_with_its_prerequisites() {
    let boxes = plan::parsed(DOC);
    let after = |id: &str| boxes.iter().find(|found| found.0 == id).map(|found| found.1.clone()).expect("box");
    assert_eq!(boxes.len(), 11);
    assert!(boxes.iter().find(|found| found.0 == "1").expect("box 1").2);
    assert_eq!(after("1"), Vec::<String>::new());
    assert_eq!(after("2b"), ["1"]);
    assert_eq!(after("3d"), ["3a", "3b", "3c"]);
    assert_eq!(after("6"), ["3d"]);
    assert_eq!(after("7"), ["1"]);
}

#[test]
fn replaying_the_morning_of_2026_11_04_finds_every_docs_box_whose_prerequisite_landed() {
    let now = DOC_ADDED + 2.0 * HOUR + 60.0;
    assert_eq!(stalled_ids(0.0, now, &[]), ids(&["2a", "2b", "3a", "3b", "3c", "4", "5", "7"]));
    assert_eq!(stalled_ids(0.0, DOC_ADDED + 2.0 * HOUR - 60.0, &[]), ids(&[]));
}

#[test]
fn replaying_the_evening_after_the_first_boxes_landed_finds_the_entry_cap_box_and_not_the_landed_ones() {
    let last_landed = LANDED.lines().filter_map(|line| line.split_once('\t')).map(|(at, _)| at.parse::<f64>().expect("epoch")).fold(0.0, f64::max);
    let now = last_landed + 2.0 * HOUR + 60.0;
    assert_eq!(stalled_ids(last_landed, now, &[]), ids(&["3d"]));
}

#[test]
fn a_lane_a_session_or_a_queued_follow_naming_the_box_keeps_it_from_being_found() {
    let now = DOC_ADDED + 3.0 * HOUR;
    let found = stalled_ids(0.0, now, &["docs-2a-router", "docs-3a-features", "sc-9-other", "follow docs-architecture box 7"]);
    assert_eq!(found, ids(&["2b", "3b", "3c", "4", "5"]));
}

#[test]
fn a_plan_finding_carries_its_owner_a_done_when_and_a_stable_key() {
    let now = DOC_ADDED + 3.0 * HOUR;
    let finding = plan::stalled(&plan_facts(0.0, &[]), now, &limits()).into_iter().find(|finding| finding.key == "plan:docs-architecture#2a").expect("box 2a");
    assert_eq!(finding.owner.as_deref(), Some("north#523"));
    assert!(finding.box_line().starts_with("- [ ] **Staff docs-architecture box 2a: Agent router (20 min).**"));
    assert!(finding.box_line().contains(" Done when: "));
    assert!(finding.box_line().ends_with("(detector plan:docs-architecture#2a)"));
    let later = plan::stalled(&plan_facts(0.0, &[]), now + HOUR, &limits());
    assert!(later.iter().any(|again| again.key == finding.key));
}

#[test]
fn an_unowned_issue_box_is_found_only_in_a_configured_repository_and_when_no_name_holds_it() {
    let now = 1_793_800_000.0;
    let issues = vec![
        IssueBox { reference: "north#700".into(), title: "x".into(), ready_since: now - 3.0 * HOUR },
        IssueBox { reference: "north#701".into(), title: "x".into(), ready_since: now - 1.0 * HOUR },
        IssueBox { reference: "north#702".into(), title: "x".into(), ready_since: now - 3.0 * HOUR },
        IssueBox { reference: "makgora#703".into(), title: "x".into(), ready_since: now - 3.0 * HOUR },
    ];
    let facts = plan::Facts { names: vec!["n702-fix".into()], issues, ..plan::Facts::default() };
    let keys: Vec<String> = plan::stalled(&facts, now, &limits()).into_iter().map(|finding| finding.key).collect();
    assert_eq!(keys, ["issue:north#700"]);
}

fn header_rows(until: f64) -> Option<f64> {
    HEADER_TIMES.lines().map(at).filter(|written| *written <= until).reduce(f64::max)
}

fn usage_signal(until: f64) -> signal::Facts {
    signal::Facts { signals: vec![Signal { name: "usage header rows".into(), writer: "token_quota.rs".into(), cadence_s: 300.0, owner: Some("north#35".into()), last_written: header_rows(until) }] }
}

#[test]
fn replaying_2026_11_04_finds_the_usage_header_rows_within_15_minutes_of_the_third_missed_write() {
    let stopped = at("2026-11-04T15:32:00Z");
    assert_eq!(header_rows(f64::MAX), Some(stopped));
    let mut tick = at("2026-11-04T15:32:00Z");
    let first = loop {
        tick += 15.0 * 60.0;
        if !signal::silent(&usage_signal(tick), tick, &limits()).is_empty() {
            break tick;
        }
    };
    let third_missed_write = stopped + 3.0 * 300.0;
    assert!(first > third_missed_write && first - third_missed_write <= 15.0 * 60.0, "found at {first}");
    let finding = &signal::silent(&usage_signal(first), first, &limits())[0];
    assert_eq!(finding.key, format!("signal:usage header rows:{}", stopped as i64));
    assert_eq!(signal::silent(&usage_signal(first + 6.0 * HOUR), first + 6.0 * HOUR, &limits())[0].key, finding.key);
}

#[test]
fn a_signal_inside_three_cadences_or_never_written_is_not_silent() {
    let now = 10_000.0;
    let signal = |last_written| Signal { name: "s".into(), writer: "w".into(), cadence_s: 100.0, owner: None, last_written };
    let facts = signal::Facts { signals: vec![signal(Some(now - 300.0)), signal(None)] };
    assert!(signal::silent(&facts, now, &limits()).is_empty());
    assert_eq!(signal::silent(&signal::Facts { signals: vec![signal(Some(now - 301.0))] }, now, &limits()).len(), 1);
}

fn refusal(lane: &str, sha: &str, minutes_ago: f64, now: f64, text: &str) -> Refusal {
    Refusal { lane: lane.into(), sha: sha.into(), at: now - minutes_ago * 60.0, text: text.into() }
}

const MARKER_RACE: &str = "test land_checks::tests::lease_released_for_actions_wait ... FAILED\n\nfailures:\n";

#[test]
fn replaying_the_etxtbsy_and_marker_race_refusals_files_one_finding_per_class_naming_every_instance() {
    let now = at("2026-11-05T01:00:00Z");
    let facts = refusal::Facts {
        owner: Some("north#360".into()),
        refusals: vec![
            refusal("job-run-a", "aaaaaaaaaaaa", 600.0, now, &format!("failed 1 blocking check(s) failed: land checks\n{BUSY_LOG}")),
            refusal("job-run-b", "bbbbbbbbbbbb", 300.0, now, &format!("failed log\n{BUSY_LOG}")),
            refusal("n360-s8", "cccccccccccc", 120.0, now, &format!("failed log\n{MARKER_RACE}")),
            refusal("n360-s10", "dddddddddddd", 60.0, now, &format!("failed log\n{MARKER_RACE}")),
            refusal("n360-s6b", "eeeeeeeeeeee", 30.0, now, "failed log\ntest other::one ... FAILED"),
            refusal("old", "ffffffffffff", 60.0 * 30.0, now, &format!("failed\n{BUSY_LOG}")),
        ],
    };
    let found = refusal::repeated(&facts, now, &limits());
    let keys: Vec<&str> = found.iter().map(|finding| finding.key.as_str()).collect();
    assert_eq!(keys, ["refusal:test:job_run_starts_its_job_and_returns_with_it_while_the_start_record_stalls:2026-11-04", "refusal:test:land_checks::tests::lease_released_for_actions_wait:2026-11-04", "refusal:text-file-busy:2026-11-04"]);
    let busy = found.iter().find(|finding| finding.key.starts_with("refusal:text-file-busy")).expect("busy");
    assert_eq!(busy.instances.len(), 2);
    assert!(busy.detail.contains("job-run-a") && busy.detail.contains("job-run-b") && !busy.detail.contains("old"));
    assert_eq!(busy.owner.as_deref(), Some("north#360"));
}

#[test]
fn two_events_of_one_push_count_once_and_a_single_refusal_is_not_a_class() {
    let now = at("2026-11-05T01:00:00Z");
    let facts = refusal::Facts {
        owner: None,
        refusals: vec![
            refusal("lane", "aaaaaaaaaaaa", 10.0, now, "CARGO_TARGET_DIR=target cargo test; log: /none.log\n"),
            refusal("lane", "aaaaaaaaaaaa", 10.0, now, &format!("1 blocking check(s) failed: land checks\n{BUSY_LOG}")),
        ],
    };
    assert!(refusal::repeated(&facts, now, &limits()).is_empty());
}

#[test]
fn a_gate_named_by_two_refusals_is_one_class() {
    let now = at("2026-11-05T01:00:00Z");
    let gate = "2 blocking check(s) failed: dependency gate; deny-list scan";
    let facts = refusal::Facts { owner: None, refusals: vec![refusal("a", "1111111111", 50.0, now, gate), refusal("b", "2222222222", 20.0, now, gate)] };
    let keys: Vec<String> = refusal::repeated(&facts, now, &limits()).into_iter().map(|finding| finding.key).collect();
    assert_eq!(keys, ["refusal:gate:deny-list scan:2026-11-05", "refusal:gate:dependency gate:2026-11-05"]);
}

struct Gh {
    body: RefCell<String>,
    state: &'static str,
    edits: RefCell<Vec<String>>,
}

impl Host for Gh {
    fn output(&self, argv: &[&str], _: Option<&Path>, input: Option<&str>, _: Duration) -> Option<Output> {
        let stdout = match argv {
            ["gh", "issue", "view", ..] => serde_json::json!({ "body": *self.body.borrow(), "state": self.state }).to_string(),
            ["gh", "issue", "edit", ..] => {
                let edited = input?.to_owned();
                *self.body.borrow_mut() = edited.clone();
                self.edits.borrow_mut().push(edited);
                String::new()
            }
            _ => return None,
        };
        let status = std::process::Command::new("true").status().ok()?;
        Some(Output { status, stdout: stdout.into_bytes(), stderr: Vec::new() })
    }
}

fn finding(key: &str, owner: Option<&str>) -> Finding {
    Finding { detector: Detector::SilentSignal, key: key.into(), owner: owner.map(str::to_owned), title: "Restore the silent signal x".into(), detail: "It stopped.".into(), done_when: "it writes.".into(), instances: vec!["x".into()] }
}

fn places(root: &Path) -> Places {
    Places { home: root.to_owned(), state: root.join("state"), ledger: root.join("threads.db") }
}

#[test]
fn a_finding_is_filed_once_as_a_box_on_the_owning_issue_and_never_again() {
    let root = tempfile::tempdir().expect("temp dir");
    let places = places(root.path());
    let gh = Gh { body: RefCell::new("Plan.\n\n- [x] done\n".into()), state: "OPEN", edits: RefCell::default() };
    let filer = Filer::new(&places, &gh);
    let one = finding("signal:x:100", Some("north#35"));
    let first = filer.file(std::slice::from_ref(&one), 1.0).expect("filed");
    assert!(first[0].filed, "{}", first[0].note);
    assert_eq!(gh.body.borrow().as_str(), "Plan.\n\n- [x] done\n- [ ] **Restore the silent signal x.** It stopped. Done when: it writes. (detector signal:x:100)\n");
    let second = filer.file(std::slice::from_ref(&one), 2.0).expect("again");
    assert!(!second[0].filed);
    assert_eq!(gh.edits.borrow().len(), 1);
    std::fs::remove_file(places.flow().join("detectors.json")).expect("state");
    let after_state_loss = filer.file(&[one], 3.0).expect("lost state");
    assert!(!after_state_loss[0].filed);
    assert_eq!(gh.edits.borrow().len(), 1);
}

#[test]
fn a_finding_without_an_owner_or_with_a_closed_owner_is_not_filed() {
    let root = tempfile::tempdir().expect("temp dir");
    let places = places(root.path());
    let closed = Gh { body: RefCell::new(String::new()), state: "CLOSED", edits: RefCell::default() };
    let outcomes = Filer::new(&places, &closed).file(&[finding("a", None), finding("b", Some("north#9"))], 1.0).expect("outcomes");
    assert!(outcomes.iter().all(|outcome| !outcome.filed));
    assert!(closed.edits.borrow().is_empty());
    assert!(outcomes[0].note.contains("no owning issue"));
}

#[test]
fn each_detector_spend_and_finding_count_lands_in_the_ledger() {
    let root = tempfile::tempdir().expect("temp dir");
    let places = places(root.path());
    let spends = [super::Spend { detector: Detector::StalledPlan, millis: 12, findings: 3 }, super::Spend { detector: Detector::SilentSignal, millis: 4, findings: 1 }];
    std::fs::create_dir_all(places.state.join("north")).expect("dir");
    super::record(&places, &spends, &[finding("signal:x:1", None)]).expect("recorded");
    let ledger = crate::ledger::Ledger::open(&places.state.join("north/ledger.db")).expect("ledger");
    let events = ledger.detector_events().expect("events");
    assert_eq!(events.len(), 2);
    assert!(events[0].contains("\"detector\":\"stalled-plan\"") && events[0].contains("\"spend_ms\":12") && events[0].contains("\"findings\":3"));
    assert!(events[1].contains("\"filed\":1"));
}

#[test]
fn the_feedback_loop_doc_declares_a_cadence_and_owner_for_the_usage_header_rows() {
    let doc = include_str!("../../../../../docs/feedback-loop.md");
    let files = crate::health::loops::declared_files(doc);
    let gate = files.iter().find(|declared| declared.name == "agents/usage-gate.json").expect("usage gate");
    assert_eq!(gate.cadence_s, 300.0);
    assert!(files.iter().all(|declared| declared.name != "agents/goal-scores.jsonl"));
    let readings = crate::health::loops::declared_readings(doc);
    assert_eq!(readings.len(), 1);
    assert_eq!((readings[0].cadence_s, readings[0].owner.as_deref()), (300.0, Some("north#35")));
}

#[test]
fn replaying_the_disk_readings_of_2026_11_05_finds_each_host_under_the_floor_once_a_day() {
    let at_2_25 = at("2026-11-05T02:25:00Z");
    let facts = disk::Facts {
        owner: Some("north#259".into()),
        hosts: vec![disk::Host { name: "gex45".into(), free_gib: 9.0 }, disk::Host { name: "laptop".into(), free_gib: 15.0 }, disk::Host { name: "nexus".into(), free_gib: 80.0 }],
    };
    let found = disk::low(&facts, at_2_25, &limits());
    assert_eq!(found.iter().map(|finding| finding.key.as_str()).collect::<Vec<_>>(), ["disk:gex45:2026-11-05"]);
    assert_eq!(found[0].owner.as_deref(), Some("north#259"));
    assert!(found[0].box_line().contains(" Done when: "));
    let later = disk::low(&facts, at_2_25 + 4.0 * HOUR, &limits());
    assert_eq!(later[0].key, found[0].key);
    assert!(disk::low(&disk::Facts::default(), at_2_25, &limits()).is_empty());
}
