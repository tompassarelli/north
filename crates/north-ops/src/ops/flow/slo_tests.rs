use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::render::format_row;
use super::slo::{self, budget_alert};
use super::tests::{Fake, at, places};

fn touch(path: &Path, content: &str, modified: f64) {
    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    fs::write(path, content).expect("file");
    OpenOptions::new().write(true).open(path).expect("open").set_modified(UNIX_EPOCH + Duration::from_secs_f64(modified)).expect("mtime");
}

fn micros(text: &str) -> u64 {
    (at(text) * 1e6) as u64
}

fn ticket(directory: &Path, area: &str, queued: &str, branch: &str, decided: Option<&str>) -> String {
    let stem = format!("{}-{branch}", micros(queued));
    let path = if area.is_empty() { directory.join(format!("{stem}.json")) } else { directory.join(area).join(format!("{stem}.json")) };
    touch(&path, &json!({"branch": branch, "verdict": decided.map(|_| "landed abc")}).to_string(), at(decided.unwrap_or(queued)));
    stem
}

fn check(directory: &Path, started: &str, name: &str, ended: &str) {
    touch(&directory.join(format!("logs/{}-{name}.log", micros(started))), "safe-push: verdict green\n", at(ended));
}

fn today(directory: &Path) {
    check(directory, "2026-11-05T09:00:00Z", "candidate", "2026-11-05T09:59:00Z");
    let waited = ticket(directory, "done", "2026-11-05T09:10:00Z", "waited-15m", Some("2026-11-05T09:50:00Z"));
    check(directory, "2026-11-05T09:25:00Z", &format!("{waited}-precheck"), "2026-11-05T09:50:00Z");
    ticket(directory, "done", "2026-11-05T09:20:00Z", "decided-22m", Some("2026-11-05T09:42:00Z"));
    let quick = ticket(directory, "done", "2026-11-05T09:30:00Z", "checked-in-5m", Some("2026-11-05T09:55:00Z"));
    check(directory, "2026-11-05T09:35:00Z", &format!("{quick}-precheck"), "2026-11-05T09:55:00Z");
    ticket(directory, "", "2026-11-05T09:40:00Z", "still-queued", None);
    for slot in 0..4 {
        check(directory, "2026-11-05T07:50:00Z", &format!("{slot}-lane-{slot}-precheck"), "2026-11-05T08:30:00Z");
    }
    ticket(directory, "done", "2026-11-05T08:00:00Z", "behind-busy-slots", Some("2026-11-05T08:45:00Z"));
    check(directory, "2026-11-05T08:31:00Z", "candidate", "2026-11-05T08:45:00Z");
    let yesterday = ticket(directory, "done", "2026-11-04T23:00:00Z", "yesterday", Some("2026-11-04T23:50:00Z"));
    check(directory, "2026-11-04T23:30:00Z", &format!("{yesterday}-precheck"), "2026-11-04T23:50:00Z");
}

fn row(directory: &Path, now: &str) -> Value {
    let (breaches, budget) = slo::reading(directory, at(now));
    json!({"repo": "north", "pending": 1, "landed_per_hour": 0, "push_to_land_minutes": 0, "push_to_land_p90_minutes": 0, "idle_slot_wait_breaches": breaches, "idle_slot_budget": budget})
}

#[test]
fn a_lane_waiting_over_ten_minutes_beside_an_idle_slot_is_a_breach_counted_on_its_day() {
    let root = tempfile::tempdir().expect("temp dir");
    today(root.path());
    let row = row(root.path(), "2026-11-05T10:00:00Z");
    let breaches: Vec<(String, i64, i64, u64)> = row["idle_slot_wait_breaches"]
        .as_array()
        .expect("breaches")
        .iter()
        .map(|breach| (breach["lane"].as_str().unwrap().to_owned(), breach["wait_minutes"].as_f64().unwrap().round() as i64, breach["idle_minutes"].as_f64().unwrap().round() as i64, breach["idle_slots"].as_u64().unwrap()))
        .collect();
    assert_eq!(breaches, vec![("waited-15m".into(), 15, 15, 3), ("decided-22m".into(), 22, 22, 3), ("still-queued".into(), 20, 20, 4)]);
    assert_eq!(row["idle_slot_wait_breaches"][0]["breached_at"], "2026-11-05T09:20:00Z");
    assert_eq!(row["idle_slot_budget"], json!({"day": "2026-11-05", "breaches": 3, "budget": 3, "spent": true, "slots": 4, "limit_minutes": 10.0}));
    let line = format_row(&row, None);
    assert!(line.contains(", idle-slot waits over 10m today 3/3 SPENT [waited-15m 15m with 3 idle; decided-22m 22m with 3 idle; still-queued 20m with 4 idle]"), "{line}");
    let early = self::row(root.path(), "2026-11-05T09:45:00Z");
    assert_eq!(early["idle_slot_budget"]["breaches"], 2, "a lane breaches only once its idle wait passes ten minutes");
    assert_eq!(slo::reading(&root.path().join("none"), at("2026-11-05T10:00:00Z")).0, Vec::new());
}

fn alarm(sends: &RefCell<Vec<String>>, rows: &[Value], now: &str, directory: &Path) {
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "list", "--json"] => Some("[]".to_owned()),
        ["north", "fleet", "send", "--from", _, node, text] => {
            sends.borrow_mut().push(format!("{node} {text}"));
            Some(String::new())
        }
        _ => None,
    };
    budget_alert(&places(directory), &Fake(&reply), rows, at(now)).expect("alert");
}

#[test]
fn the_day_budget_spent_sends_one_message_to_lander_lead() {
    let queue = tempfile::tempdir().expect("queue");
    let state = tempfile::tempdir().expect("state");
    today(queue.path());
    let sends = RefCell::new(Vec::new());
    alarm(&sends, &[row(queue.path(), "2026-11-05T09:45:00Z")], "2026-11-05T09:45:00Z", state.path());
    assert!(sends.borrow().is_empty(), "two breaches leave the budget of three unspent");
    for now in ["2026-11-05T10:00:00Z", "2026-11-05T10:15:00Z", "2026-11-05T23:45:00Z"] {
        alarm(&sends, &[row(queue.path(), now)], now, state.path());
    }
    let sent = sends.borrow().clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("lander-lead Lander SLO budget spent in north on 2026-11-05: 3 lanes waited over 10m while a check slot was idle (budget 3 a day, 4 slots): waited-15m waited 15m, 15m of it with 3 idle;"), "{}", sent[0]);
    alarm(&sends, &[row(queue.path(), "2026-11-06T00:15:00Z")], "2026-11-06T00:15:00Z", state.path());
    assert_eq!(sends.borrow().len(), 1, "a new day starts with its budget unspent");
}
