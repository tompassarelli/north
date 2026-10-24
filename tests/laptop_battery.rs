use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;

fn laptop(root: &Path, minute: u32, arguments: &[&str]) -> Value {
    let output = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/laptop"))
        .args(arguments)
        .env("THREADS_DB", root.join("threads.db"))
        .env("LAPTOP_THREADS", "sh -c 'echo []'")
        .env("LAPTOP_STATE", root.join("laptop.json"))
        .env("LAPTOP_WINDOW_FILE", root.join("window.json"))
        .env("LAPTOP_NOW", format!("2026-10-24T{:02}:{:02}:00+00:00", 16 + minute / 60, minute % 60))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_str(&fs::read_to_string(root.join("laptop.json")).unwrap()).unwrap()
}

fn observe(root: &Path, minute: u32, battery_low: bool) -> Value {
    let sample = json!({ "gaming": false, "agent_gpu": false, "idle": true, "gpu_busy": null, "battery_low": battery_low });
    laptop(root, minute, &["observe", &sample.to_string()])
}

fn held(state: &Value, expected: &str, reason: &str) {
    assert_eq!((state["state"].as_str(), state["reason"].as_str()), (Some(expected), Some(reason)), "{state}");
}

#[test]
fn a_low_battery_drains_the_laptop_until_ac_returns_inside_and_outside_the_window() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    fs::write(root.join("window.json"), json!({ "tz": "UTC", "open": "16:00", "close": "18:00" }).to_string()).unwrap();
    laptop(root, 0, &["open"]);

    let drained = observe(root, 1, true);
    held(&drained, "draining", "battery");
    assert_eq!(drained["grace"], 900);
    assert_eq!(observe(root, 2, true)["since"], drained["since"]);
    held(&laptop(root, 2, &["tick"]), "unavailable", "drained (battery)");
    held(&observe(root, 40, true), "unavailable", "drained (battery)");
    held(&observe(root, 41, false), "available", "window");

    held(&observe(root, 50, true), "draining", "battery");
    laptop(root, 50, &["close", "manual"]);
    held(&observe(root, 51, false), "draining", "manual");

    laptop(root, 52, &["open"]);
    held(&observe(root, 53, true), "draining", "battery");
    held(&observe(root, 130, false), "unavailable", "window");
}
