use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn transcript(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        format!(r#"{{"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"s","message":{{"content":"{text}"}}}}"#),
        r#"{"type":"assistant","timestamp":"2026-10-08T00:04:00Z","requestId":"r1","message":{"model":"claude-opus-5-5","usage":{"output_tokens":50,"input_tokens":700},"content":[{"type":"text","text":"Done: landed."}]}}"#.to_owned(),
    ];
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

#[test]
fn claude_runs_lists_the_fleet_once_and_records_only_transcripts_with_a_fleet_role() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom");
    transcript(&projects.join("proxy.jsonl"), "Item: north#1");
    for n in 0..5 {
        transcript(&projects.join(format!("probe-{n}.jsonl")), "probe");
    }
    let calls = root.join("fleet-calls");
    let fleet = root.join("north");
    fs::write(
        &fleet,
        format!("#!/bin/sh\necho call >> {}\nprintf '%s' '[{{\"name\":\"p\",\"conversation\":\"proxy\",\"parent\":null}}]'\n", calls.display()),
    )
    .unwrap();
    fs::set_permissions(&fleet, fs::Permissions::from_mode(0o755)).unwrap();
    let empty = root.join("empty");
    fs::write(&empty, "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["ledger", "claude-runs", "--since", "2026-10-08"])
        .arg("--settled")
        .arg(&empty)
        .arg("--recorded")
        .arg(&empty)
        .env("HOME", root.join("home"))
        .env("NORTH_BIN", &fleet)
        .env("AGENTS_ORCHESTRATION", root.join("absent.toml"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let rows: Vec<Value> = String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0]["id"].as_str(), rows[0]["role"].as_str(), rows[0]["outcome"].as_str()), (Some("proxy"), Some("proxy"), Some("done")));
    assert_eq!(fs::read_to_string(&calls).unwrap().lines().count(), 1);
}
