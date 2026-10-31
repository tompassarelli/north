use std::fs;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Chrome, read, run_chrome};

const SCENARIOS: &str = include_str!("fixtures/scenarios.json");

#[test]
fn recorded_readings_leave_the_cache_and_say_what_the_old_tool_did() {
    let scenarios: Vec<Value> = serde_json::from_str(SCENARIOS).unwrap();
    assert_eq!(scenarios.len(), 26);
    for scenario in &scenarios {
        let name = scenario["name"].as_str().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let cache = scratch.path().join("agents/usage/claude-web.json");
        let orchestration = scratch.path().join("orchestration.toml");
        if let Some(text) = scenario["orchestration"].as_str() {
            fs::write(&orchestration, text).unwrap();
        }
        let before = scenario["raw_cache_before"].as_str().map(str::to_owned).or_else(|| (!scenario["cache_before"].is_null()).then(|| scenario["cache_before"].to_string()));
        if let Some(text) = &before {
            fs::create_dir_all(cache.parent().unwrap()).unwrap();
            fs::write(&cache, text).unwrap();
        }
        let chrome = match scenario["chrome_rc"].as_i64().unwrap() {
            0 => Chrome::Reply(scenario["chrome_stdout"].as_str().unwrap().to_owned()),
            _ => Chrome::Failed,
        };
        let outcome = read(&cache, &orchestration, chrome);
        let expected = &scenario["expected"];
        let message = outcome.err().unwrap_or("");
        let want_stderr = expected["stderr"].as_str().unwrap();
        assert_eq!(message, want_stderr, "{name}");
        assert_eq!(expected["rc"].as_i64().unwrap(), i64::from(!message.is_empty()), "{name}");
        let after: Value = match fs::read_to_string(&cache) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| Value::String("<unparseable>".into())),
            Err(_) => Value::Null,
        };
        assert_eq!(after, expected["cache_after"], "{name}");
    }
}

#[test]
fn a_chrome_that_outlives_its_timeout_is_killed_and_reported_as_timed_out() {
    let started = Instant::now();
    let outcome = run_chrome("sleep", Duration::from_millis(100));
    assert!(matches!(outcome, Chrome::Failed), "sleep rejects the tool argument and exits at once");
    let scratch = tempfile::tempdir().unwrap();
    let slow = scratch.path().join("slow-chrome");
    let written = std::process::Command::new("sh")
        .args(["-c", r#"printf '#!/bin/sh\nsleep 30\n' > "$1" && chmod 755 "$1""#, "sh"])
        .arg(&slow)
        .status()
        .unwrap();
    assert!(written.success());
    assert!(matches!(run_chrome(slow.to_str().unwrap(), Duration::from_millis(200)), Chrome::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(10));
}
