use super::habit_notes;

#[test]
fn the_edit_hook_tells_the_agent_the_rewrite_and_warns_on_a_break_with_no_fix() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(std::process::Command::new("git").arg("-C").arg(root).args(["init", "-q"]).status().unwrap().success());
    std::fs::write(root.join("structure.toml"), "[rust_habits]\nchecks = [\"ambient-test-input\"]\n[[rust_habits.ambient_inputs]]\ninput = \"clock\"\nallowlist = \"clocks.tsv\"\nfixture = \"inject the clock\"\n[[rust_habits.ambient_inputs]]\ninput = \"host\"\nallowlist = \"hosts.tsv\"\nfixture = \"pin the reading\"\n").unwrap();
    std::fs::write(root.join("clocks.tsv"), "").unwrap();
    std::fs::write(root.join("hosts.tsv"), "").unwrap();
    std::fs::create_dir(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/aged.rs"), "#[test]\nfn t() {\n    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);\n}\n").unwrap();
    std::fs::write(root.join("tests/waits.rs"), "#[test]\nfn t() {\n    std::thread::sleep(d);\n}\n").unwrap();
    std::fs::write(root.join("tests/host.rs"), "#[test]\nfn t() {\n    rustix::fs::statvfs(p);\n}\n").unwrap();

    let aged = habit_notes(root, "tests/aged.rs");
    assert_eq!(aged.len(), 1);
    assert!(aged[0].contains("ambient-test-input rewrote tests/aged.rs:3"), "{aged:?}");
    assert!(aged[0].contains("Before: `let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);`"), "{aged:?}");
    assert!(aged[0].contains("After: `let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000) - std::time::Duration::from_secs(60);` Write this form next time."), "{aged:?}");

    let waits = habit_notes(root, "tests/waits.rs");
    assert_eq!(waits.len(), 1);
    assert!(waits[0].starts_with("habits: fix these before your next commit"), "{waits:?}");
    assert!(waits[0].contains("ambient-test-input tests/waits.rs:3 thread::sleep in test code"), "{waits:?}");

    let host = habit_notes(root, "tests/host.rs");
    assert_eq!(host.len(), 1);
    assert!(host[0].contains("statvfs in test code reads the real host"), "{host:?}");
    assert!(host[0].contains("tests/host.rs:3"), "{host:?}");
}
