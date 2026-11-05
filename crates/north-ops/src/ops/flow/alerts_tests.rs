use std::cell::RefCell;

use serde_json::{Value, json};

use super::alerts::stall_alert;
use super::tests::{Fake, places};

const NOW: f64 = 1_793_300_000.0;
const QUARTER: f64 = 900.0;

fn stalled(job: u64, minutes: f64) -> Value {
    json!({
        "stalled": true, "reason": format!("autoland.yml run {job} reports running {minutes:.1}m > 6.0m on vast-1"), "main_age_minutes": minutes,
        "main_limit_minutes": 30.0, "median_gap_minutes": null, "oldest_pending_minutes": null, "runner": "vast-1",
        "overdue_jobs": [{"workflow": "autoland.yml", "run_id": job, "name": "reports", "started_at": NOW, "runner": "vast-1", "pool": null, "age_minutes": minutes, "limit_minutes": 6.0}],
    })
}

fn healthy() -> Value {
    json!({"stalled": false, "reason": "healthy", "main_age_minutes": 1.0, "main_limit_minutes": 30.0, "median_gap_minutes": null, "oldest_pending_minutes": null, "overdue_jobs": [], "runner": null})
}

fn row(stall: Value, stranded: Value, refused: Value) -> Value {
    json!({"repo": "makgora", "stall": stall, "stranded": stranded, "refused": refused})
}

fn lane(minutes: f64) -> Value {
    json!({"session": "n-b245", "lane": "citations", "stop": "capacity", "machine": null, "minutes": minutes})
}

fn refusal(minutes: f64) -> Value {
    json!({"branch": "claude/land-x", "session": null, "reason": "rebase conflicts in a.rs", "url": "https://example.test/run/1", "minutes": minutes})
}

fn run(sends: &RefCell<Vec<String>>, rows: &[Value], at: f64, directory: &std::path::Path) {
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "list", "--json"] => Some("[]".to_owned()),
        ["north", "fleet", "send", "--from", _, node, text] => {
            sends.borrow_mut().push(format!("{node} {text}"));
            Some(String::new())
        }
        _ => None,
    };
    stall_alert(&places(directory), &Fake(&reply), rows, at).unwrap();
}

fn episode(rows: impl Fn(f64) -> Value, kind: &str) {
    let directory = tempfile::tempdir().unwrap();
    let sends = RefCell::new(Vec::<String>::new());
    let count = || sends.borrow().iter().filter(|message| message.contains(kind)).count();
    run(&sends, &[rows(1.0)], NOW - 300.0, directory.path());
    assert_eq!(count(), 0, "{kind} seen by a single run is not sent");
    run(&sends, &[rows(1.0)], NOW, directory.path());
    assert_eq!(count(), 2, "{kind} seen by two consecutive runs reaches the parent and the proxy once");
    run(&sends, &[rows(16.0)], NOW + QUARTER, directory.path());
    run(&sends, &[rows(31.0)], NOW + 2.0 * QUARTER, directory.path());
    assert_eq!(count(), 2, "{kind} unchanged 15 minutes later is not sent again");
    run(&sends, &[row(healthy(), json!([]), json!([]))], NOW + 3.0 * QUARTER, directory.path());
    run(&sends, &[rows(1.0)], NOW + 4.0 * QUARTER, directory.path());
    assert_eq!(count(), 2, "{kind} that returned for a single run is not sent");
    run(&sends, &[rows(6.0)], NOW + 5.0 * QUARTER, directory.path());
    assert_eq!(count(), 4, "{kind} that cleared and returned for two runs is sent again");
}

#[test]
fn test_stall_alert_is_sent_once_per_episode() {
    episode(|minutes| row(stalled(7, minutes), json!([]), json!([])), "Landing stalled");
}

#[test]
fn test_stranded_alert_is_sent_once_per_episode() {
    episode(|minutes| row(healthy(), json!([lane(minutes)]), json!([])), "Lanes stranded");
}

#[test]
fn test_refused_alert_is_sent_once_per_episode() {
    episode(|minutes| row(healthy(), json!([]), json!([refusal(minutes)])), "Autoland refused");
}

#[test]
fn test_a_new_job_or_lane_in_the_same_stall_opens_a_new_episode() {
    let directory = tempfile::tempdir().unwrap();
    let sends = RefCell::new(Vec::<String>::new());
    let tick = |stall: Value, stranded: Value, at: f64| run(&sends, &[row(stall, stranded, json!([]))], at, directory.path());
    tick(stalled(7, 10.0), json!([]), NOW);
    tick(stalled(7, 15.0), json!([]), NOW + 300.0);
    assert_eq!(sends.borrow().len(), 2);
    tick(stalled(8, 10.0), json!([]), NOW + 600.0);
    assert_eq!(sends.borrow().len(), 2, "a new job seen by a single run is not sent");
    tick(stalled(8, 15.0), json!([]), NOW + 900.0);
    assert_eq!(sends.borrow().len(), 4);
    tick(stalled(8, 40.0), json!([lane(20.0)]), NOW + 1200.0);
    tick(stalled(8, 45.0), json!([lane(25.0)]), NOW + 1500.0);
    assert_eq!(sends.borrow().len(), 6, "the stranded lane is new, the unchanged stall is not repeated");
}

#[test]
fn a_lander_stall_alerts_once_per_episode_under_its_own_identity() {
    let healthy: super::stall::Verdict = serde_json::from_value(healthy()).expect("verdict");
    let reasons = vec!["lander tickets queued: 12 waiting, oldest 116m, and main has not moved through the lander for 116m > 20m".to_owned()];
    let verdict = serde_json::to_value(super::stall::with_lander(healthy, &reasons)).expect("json");
    let rows = [row(verdict, json!([]), json!([]))];
    let (mut state, first) = super::tests::second_run(&rows, NOW, &[]);
    assert_eq!(first.len(), 2);
    for action in &first {
        state.get_mut("makgora").expect("record").sent.insert(action.key.clone(), NOW);
    }
    assert!(first[0].text.starts_with("Landing stalled in makgora: lander tickets queued: 12 waiting"), "{}", first[0].text);
    assert!(first[0].key.starts_with("notice:"));
    let (_, again) = super::alerts::alert_plan(&rows, &state, NOW + QUARTER, &[]);
    assert!(again.is_empty());
}
