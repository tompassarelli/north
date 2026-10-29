use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;

fn at(minute: u32) -> String {
    let total = 16 * 60 + minute;
    format!("2026-10-{:02}T{:02}:{:02}:00+00:00", 24 + total / 1440, total / 60 % 24, total % 60)
}

fn laptop(root: &Path, minute: u32, arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .arg("laptop")
        .args(arguments)
        .env("THREADS_DB", root.join("threads.db"))
        .env("LAPTOP_THREADS", "sh -c 'echo []'")
        .env("LAPTOP_STATE", root.join("laptop.json"))
        .env("LAPTOP_WINDOW_FILE", root.join("window.json"))
        .env("LAPTOP_NOW", at(minute))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_str(&fs::read_to_string(root.join("laptop.json")).unwrap()).unwrap()
}

fn observe(root: &Path, minute: u32, observed: Value) -> Value {
    let mut sample = json!({ "gaming": false, "agent_gpu": false, "idle": true, "gpu_busy": null, "sleeping": false, "battery": null, "battery_low": false });
    for (key, value) in observed.as_object().unwrap() {
        sample[key] = value.clone();
    }
    sample["battery_low"] = json!(sample["battery"].as_u64().is_some_and(|percent| percent < 30));
    laptop(root, minute, &["observe", &sample.to_string()])
}

fn held(state: &Value, expected: &str, reason: &str) {
    assert_eq!((state["state"].as_str(), state["reason"].as_str()), (Some(expected), Some(reason)), "{state}");
}

fn windowed() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("window.json"), json!({ "tz": "UTC", "open": "16:00", "close": "18:00" }).to_string()).unwrap();
    laptop(root.path(), 0, &["open"]);
    root
}

#[test]
fn a_low_battery_drains_the_laptop_until_ac_returns_inside_and_outside_the_window() {
    let root = windowed();
    let root = root.path();
    let low = json!({ "battery": 25 });

    let drained = observe(root, 1, low.clone());
    held(&drained, "draining", "battery");
    assert_eq!(drained["grace"], 900);
    assert_eq!(observe(root, 2, low.clone())["since"], drained["since"]);
    held(&laptop(root, 2, &["tick"]), "unavailable", "drained (battery)");
    held(&observe(root, 40, low.clone()), "unavailable", "drained (battery)");
    held(&observe(root, 41, json!({ "battery": 30 })), "unavailable", "drained (battery)");
    held(&observe(root, 42, json!({})), "available", "window");

    held(&observe(root, 50, low.clone()), "draining", "battery");
    laptop(root, 50, &["close", "manual"]);
    held(&observe(root, 51, json!({})), "draining", "manual");

    laptop(root, 52, &["open"]);
    held(&observe(root, 53, low.clone()), "draining", "battery");
    laptop(root, 54, &["tick"]);
    held(&laptop(root, 1445, &["tick"]), "unavailable", "drained (battery)");
    held(&observe(root, 1446, json!({})), "available", "window");
    held(&observe(root, 1447, low), "draining", "battery");
    held(&observe(root, 1570, json!({})), "unavailable", "window");
}

#[test]
fn a_sleep_closes_only_an_open_laptop_and_waking_restores_it() {
    let root = windowed();
    let root = root.path();
    let asleep = json!({ "sleeping": true });

    let slept = observe(root, 1, asleep.clone());
    held(&slept, "unavailable", "sleep");
    assert_eq!(slept["grace"], 120);
    held(&observe(root, 2, json!({})), "available", "window");
    held(&observe(root, 2, json!({ "sleeping": true, "battery": 50 })), "unavailable", "sleep");
    held(&observe(root, 3, json!({ "battery": 50 })), "available", "window");

    laptop(root, 3, &["close", "manual"]);
    held(&observe(root, 4, asleep.clone()), "draining", "manual");
    held(&observe(root, 5, json!({})), "draining", "manual");

    laptop(root, 6, &["open"]);
    held(&observe(root, 7, json!({ "sleeping": true, "battery": 25 })), "unavailable", "sleep");
    held(&observe(root, 8, json!({ "battery": 25 })), "draining", "battery");

    laptop(root, 9, &["open"]);
    for minute in 10..15 {
        observe(root, minute, json!({ "idle": false }));
    }
    held(&observe(root, 15, json!({ "idle": false })), "draining", "active");
    held(&observe(root, 16, json!({ "sleeping": true, "idle": false })), "draining", "active");
    held(&observe(root, 17, json!({ "idle": false })), "draining", "active");

    laptop(root, 18, &["open"]);
    held(&observe(root, 19, asleep.clone()), "unavailable", "sleep");
    held(&laptop(root, 125, &["tick"]), "unavailable", "sleep");
    held(&laptop(root, 1445, &["tick"]), "unavailable", "sleep");
    held(&observe(root, 1446, json!({})), "available", "window");
}

#[test]
fn waking_keeps_an_open_until_word_and_its_expiry() {
    let root = windowed();
    let root = root.path();
    laptop(root, 130, &["open", "until", "20:00"]);
    let slept = observe(root, 131, json!({ "sleeping": true }));
    held(&slept, "unavailable", "sleep");
    assert_eq!(slept["until"], "2026-10-24T20:00:00Z");
    let woke = observe(root, 132, json!({}));
    held(&woke, "available", "window");
    assert_eq!(woke["until"], "2026-10-24T20:00:00Z");
    held(&laptop(root, 241, &["tick"]), "unavailable", "drained (window)");

    laptop(root, 250, &["open", "until", "20:30"]);
    observe(root, 251, json!({ "sleeping": true }));
    held(&observe(root, 280, json!({})), "unavailable", "window");
}

fn idle_through(root: &Path, minutes: std::ops::Range<u32>, observed: &Value) -> Value {
    minutes.fold(Value::Null, |_, minute| observe(root, minute, observed.clone()))
}

#[test]
fn a_seat_idle_on_power_reopens_the_laptop_after_the_window_closes_it_and_tom_active_or_on_battery_keeps_it_shut() {
    let root = windowed();
    let root = root.path();
    held(&laptop(root, 120, &["tick"]), "unavailable", "drained (window)");

    held(&idle_through(root, 122..152, &json!({})), "unavailable", "drained (window)");
    held(&observe(root, 152, json!({})), "available", "idle");

    held(&idle_through(root, 153..158, &json!({ "idle": false })), "available", "idle");
    held(&observe(root, 158, json!({ "idle": false })), "draining", "active");
    held(&laptop(root, 159, &["tick"]), "unavailable", "drained (active)");

    held(&idle_through(root, 160..200, &json!({ "battery": 80 })), "unavailable", "drained (active)");
    held(&observe(root, 200, json!({})), "available", "idle");

    laptop(root, 201, &["close", "manual"]);
    held(&laptop(root, 202, &["tick"]), "unavailable", "drained (manual)");
    held(&idle_through(root, 203..270, &json!({})), "unavailable", "drained (manual)");
}
