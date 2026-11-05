use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::Ctx;

const THREADS: &str = "sh -c 'echo []'";

fn at(minute: u32) -> String {
    let total = 16 * 60 + minute;
    format!("2026-10-{:02}T{:02}:{:02}:00+00:00", 24 + total / 1440, total / 60 % 24, total % 60)
}

fn laptop(root: &Path, minute: u32, arguments: &[&str]) -> Value {
    let arguments: Vec<String> = arguments.iter().map(|word| (*word).to_owned()).collect();
    if let Err(die) = Ctx::fixture(root, &at(minute), THREADS).main(&arguments) {
        panic!("laptop {arguments:?} at minute {minute}: {} ({})", die.message, die.code);
    }
    serde_json::from_str(&fs::read_to_string(root.join("placement/laptop.json")).unwrap()).unwrap()
}

fn observe(root: &Path, minute: u32, observed: &Value) -> Value {
    let mut sample = json!({ "gaming": false, "idle": true, "sleeping": false, "battery": null });
    for (key, value) in observed.as_object().unwrap() {
        sample[key] = value.clone();
    }
    laptop(root, minute, &["observe", &sample.to_string()])
}

fn idle_through(root: &Path, minutes: std::ops::Range<u32>, observed: &Value) -> Value {
    minutes.fold(Value::Null, |_, minute| observe(root, minute, observed))
}

fn held(state: &Value, expected: &str, reason: &str) {
    assert_eq!((state["state"].as_str(), state["reason"].as_str()), (Some(expected), Some(reason)), "{state}");
}

#[test]
fn a_seat_idle_on_power_reopens_the_laptop_after_the_window_closes_it_and_tom_active_or_on_battery_keeps_it_shut() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::write(root.join("window.json"), json!({ "tz": "UTC", "open": "16:00", "close": "18:00" }).to_string()).unwrap();
    laptop(root, 0, &["open"]);
    held(&laptop(root, 120, &["tick"]), "unavailable", "drained (window)");

    held(&idle_through(root, 122..152, &json!({})), "unavailable", "drained (window)");
    held(&observe(root, 152, &json!({})), "available", "idle");

    held(&idle_through(root, 153..158, &json!({ "idle": false })), "available", "idle");
    held(&observe(root, 158, &json!({ "idle": false })), "draining", "active");
    held(&laptop(root, 159, &["tick"]), "unavailable", "drained (active)");

    held(&idle_through(root, 160..200, &json!({ "battery": 80 })), "draining", "battery");
    held(&observe(root, 200, &json!({})), "unavailable", "window");
    held(&observe(root, 201, &json!({})), "available", "idle");

    laptop(root, 201, &["close", "manual"]);
    held(&laptop(root, 202, &["tick"]), "unavailable", "drained (manual)");
    held(&idle_through(root, 203..270, &json!({})), "unavailable", "drained (manual)");
}
