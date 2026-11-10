use super::{evaluate, habit_notes};
use crate::hook_input::{HookInput, Verdict};

fn clock() -> String {
    ["std::time::", "System", "Time", "::now"].concat()
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(std::process::Command::new("git").arg("-C").arg(root).args(["init", "-q"]).status().unwrap().success());
    std::fs::write(root.join("structure.toml"), "[rust_habits]\nchecks = [\"ambient-test-input\"]\n[[rust_habits.ambient_inputs]]\ninput = \"clock\"\nallowlist = \"clocks.tsv\"\nfixture = \"inject the clock\"\n[[rust_habits.ambient_inputs]]\ninput = \"host\"\nallowlist = \"hosts.tsv\"\nfixture = \"pin the reading\"\n").unwrap();
    std::fs::write(root.join("clocks.tsv"), "").unwrap();
    std::fs::write(root.join("hosts.tsv"), "").unwrap();
    std::fs::create_dir(root.join("tests")).unwrap();
    let clock = clock();
    let sleep = ["std::thread::", "sleep", "(d);"].concat();
    std::fs::write(root.join("tests/aged.rs"), format!("#[test]\nfn t() {{\n    let old = {clock}() - std::time::Duration::from_secs(60);\n}}\n")).unwrap();
    std::fs::write(root.join("tests/waits.rs"), format!("#[test]\nfn t() {{\n    {sleep}\n}}\n")).unwrap();
    std::fs::write(root.join("tests/host.rs"), "#[test]\nfn t() {\n    rustix::fs::statvfs(p);\n}\n").unwrap();
    std::fs::write(root.join("tests/mixed.rs"), format!("#[test]\nfn t() {{\n    let old = {clock}() - std::time::Duration::from_secs(60);\n    rustix::fs::statvfs(p);\n}}\n")).unwrap();
    std::fs::write(root.join("tests/unmatched.rs"), format!("#[test]\nfn t() {{\n    let now = {clock}();\n}}\n")).unwrap();
    dir
}

#[test]
fn the_post_tool_use_edit_guard_leaves_the_file_unchanged_and_advises_the_fix() {
    let dir = fixture();
    let aged = dir.path().join("tests/aged.rs");
    let before = std::fs::read_to_string(&aged).unwrap();
    let payload = serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Edit", "tool_input": {"file_path": "tests/aged.rs"}, "cwd": dir.path()});
    let verdict = evaluate(&HookInput::parse(payload.to_string().as_bytes()).unwrap());
    let Verdict::Advise(advice) = verdict else { panic!("the edit guard did not advise: {verdict:?}") };
    let clock = clock();
    assert!(advice.contains("ambient-test-input wants tests/aged.rs:3 changed"), "{advice}");
    assert!(advice.contains(&format!("Before: `let old = {clock}() - std::time::Duration::from_secs(60);`")), "{advice}");
    assert!(advice.contains("After: `let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000) - std::time::Duration::from_secs(60);`"), "{advice}");
    assert_eq!(std::fs::read_to_string(&aged).unwrap(), before, "the hook rewrote the file the agent must still match its next edit against");
}

#[test]
fn the_edit_hook_reports_a_mechanical_habit_fix_without_touching_the_file_and_warns_on_a_break_with_no_fix() {
    let dir = fixture();
    let root = dir.path();
    let clock = clock();

    let aged_text = std::fs::read_to_string(root.join("tests/aged.rs")).unwrap();
    let aged = habit_notes(root, "tests/aged.rs");
    assert_eq!(aged.len(), 1);
    assert!(aged[0].contains("ambient-test-input wants tests/aged.rs:3 changed"), "{aged:?}");
    assert!(aged[0].contains(&format!("Before: `let old = {clock}() - std::time::Duration::from_secs(60);`")), "{aged:?}");
    assert!(aged[0].contains("After: `let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000) - std::time::Duration::from_secs(60);`"), "{aged:?}");
    assert!(aged[0].contains("the hook never edits your file"), "{aged:?}");
    assert_eq!(std::fs::read_to_string(root.join("tests/aged.rs")).unwrap(), aged_text, "the hook rewrote the file the agent must still match its next edit against");

    let waits = habit_notes(root, "tests/waits.rs");
    assert_eq!(waits.len(), 1);
    assert!(waits[0].starts_with("habits: fix these before your next commit"), "{waits:?}");
    assert!(waits[0].contains("ambient-test-input tests/waits.rs:3 thread::sleep in test code"), "{waits:?}");

    let host = habit_notes(root, "tests/host.rs");
    assert_eq!(host.len(), 1);
    assert!(host[0].contains("statvfs in test code reads the real host"), "{host:?}");
    assert!(host[0].contains("tests/host.rs:3"), "{host:?}");

    let mixed_text = std::fs::read_to_string(root.join("tests/mixed.rs")).unwrap();
    let mixed = habit_notes(root, "tests/mixed.rs");
    assert_eq!(mixed.len(), 2, "{mixed:?}");
    assert!(mixed[0].contains("ambient-test-input wants tests/mixed.rs:3 changed"), "{mixed:?}");
    assert!(mixed[1].starts_with("habits: fix these before your next commit"), "{mixed:?}");
    assert!(mixed[1].contains("tests/mixed.rs:4 statvfs in test code reads the real host"), "{mixed:?}");
    assert!(!mixed[1].contains(&clock), "the warning repeated the finding the suggestion already names: {mixed:?}");
    assert_eq!(std::fs::read_to_string(root.join("tests/mixed.rs")).unwrap(), mixed_text, "the hook rewrote the file the agent must still match its next edit against");

    let unmatched = habit_notes(root, "tests/unmatched.rs");
    assert_eq!(unmatched.len(), 1, "{unmatched:?}");
    assert!(unmatched[0].starts_with("habits: fix these before your next commit"), "{unmatched:?}");
    let item = ["System", "Time", "::", "now"].concat();
    assert!(unmatched[0].contains(&format!("tests/unmatched.rs:3 {item}")), "{unmatched:?}");
}
