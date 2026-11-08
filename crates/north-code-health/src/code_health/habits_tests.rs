use super::habits::findings;
use std::path::Path;

#[test]
fn the_north_tree_keeps_its_rust_habits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = findings(&root, None).unwrap();
    assert!(found.is_empty(), "{}", found.join("\n"));
    a_test_reading_the_host_outside_the_fixture_beyond_its_listed_count_is_named_and_a_stale_count_is_refused();
    a_test_reading_the_wall_clock_or_sleeping_outside_the_fixture_is_named_at_each_site();
}

fn a_test_reading_the_host_outside_the_fixture_beyond_its_listed_count_is_named_and_a_stale_count_is_refused() {
    use super::habits::{Habits, check, files_of};
    let config = r#"
[rust_habits]
checks = ["ambient-test-input"]
[[rust_habits.ambient_inputs]]
input = "host"
allowlist = "list.tsv"
fixtures = ["tests/fixture.rs"]
fixture = "pin the reading"
"#;
    let mut habits = Habits::parse(config).unwrap().unwrap();
    habits.read_ambient(|_| Ok("tests/listed.rs\t1\ntests/shrunk.rs\t2\n".to_owned())).unwrap();
    let reads = "#[test]\nfn t() {\n    rustix::fs::statvfs(p);\n}\n";
    let sources = [
        ("tests/new.rs", reads.to_owned()),
        ("tests/listed.rs", reads.to_owned()),
        ("tests/shrunk.rs", reads.to_owned()),
        ("tests/fixture.rs", reads.to_owned()),
        ("src/host.rs", "fn live() {\n    statvfs(p);\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        rustix::fs::statvfs(p);\n    }\n}\n".to_owned()),
    ]
    .map(|(path, text)| (path.to_owned(), text))
    .to_vec();
    let files = files_of(&habits, sources);
    let found: Vec<String> = check(&habits, &files, None).iter().map(|finding| format!("{}:{}", finding.path, finding.line)).collect();
    assert_eq!(found, ["tests/new.rs:3", "src/host.rs:7", "tests/shrunk.rs:0"]);
}

fn a_test_reading_the_wall_clock_or_sleeping_outside_the_fixture_is_named_at_each_site() {
    use super::habits::{Habits, check, files_of};
    let config = r#"
[rust_habits]
checks = ["ambient-test-input"]
[[rust_habits.ambient_inputs]]
input = "clock"
allowlist = "clocks.tsv"
fixtures = ["tests/clock.rs"]
fixture = "inject the clock"
"#;
    let mut habits = Habits::parse(config).unwrap().unwrap();
    habits.read_ambient(|_| Ok("tests/listed.rs\t1\n".to_owned())).unwrap();
    let racy = "use std::thread::sleep;\n#[test]\nfn t() {\n    let start = std::time::Instant::now();\n    std::thread::sleep(d);\n    tokio::time::sleep(d).await;\n    SystemTime :: now();\n    clock.sleep(d);\n    run(\"sleep 1; Instant::now()\");\n}\n";
    let sources = [
        ("tests/racy.rs", racy.to_owned()),
        ("tests/clock.rs", racy.to_owned()),
        ("tests/listed.rs", "#[test]\nfn t() {\n    let start = Instant::now();\n}\n".to_owned()),
        ("src/live.rs", "fn live() {\n    std::thread::sleep(d);\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let _ = std::time::Instant::now;\n    }\n}\n".to_owned()),
    ]
    .map(|(path, text)| (path.to_owned(), text))
    .to_vec();
    let files = files_of(&habits, sources);
    let found: Vec<String> = check(&habits, &files, None).iter().map(|finding| format!("{}:{} {}", finding.path, finding.line, finding.item)).collect();
    assert_eq!(found, ["tests/racy.rs:1 thread::sleep", "tests/racy.rs:4 Instant::now", "tests/racy.rs:5 thread::sleep", "tests/racy.rs:6 time::sleep", "tests/racy.rs:7 SystemTime::now", "src/live.rs:7 Instant::now"]);
}

#[test]
fn a_wall_clock_age_in_test_code_is_rewritten_to_a_fixed_moment_and_a_poll_loop_is_left_for_the_agent() {
    use super::habits::fix_file;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(root).args(args).status().unwrap().success());
    git(&["init", "-q"]);
    std::fs::write(root.join("structure.toml"), "[rust_habits]\nchecks = [\"ambient-test-input\"]\n[[rust_habits.ambient_inputs]]\ninput = \"clock\"\nallowlist = \"clocks.tsv\"\nfixture = \"inject the clock\"\n").unwrap();
    std::fs::write(root.join("clocks.tsv"), "").unwrap();
    std::fs::create_dir(root.join("tests")).unwrap();
    let denylist = "#[test]\nfn stale() {\n    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);\n    set(hour_ago);\n}\n";
    let polling = "#[test]\nfn ready() {\n    let started = Instant::now();\n    std::thread::sleep(READY_POLL);\n    let _ = SystemTime::now();\n}\n";
    std::fs::write(root.join("tests/denylist.rs"), denylist).unwrap();
    std::fs::write(root.join("tests/polling.rs"), polling).unwrap();

    let rewrites = fix_file(root, "tests/denylist.rs").unwrap();
    assert_eq!(rewrites.len(), 1);
    assert_eq!(rewrites[0].rule, "ambient-test-input");
    assert_eq!(rewrites[0].line, 3);
    assert_eq!(rewrites[0].before, "let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);");
    assert_eq!(rewrites[0].after, "let hour_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000) - std::time::Duration::from_secs(3600);");
    assert!(std::fs::read_to_string(root.join("tests/denylist.rs")).unwrap().contains(&rewrites[0].after));
    assert!(super::habits::findings(root, Some("tests/denylist.rs")).unwrap().is_empty());

    assert!(fix_file(root, "tests/polling.rs").unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("tests/polling.rs")).unwrap(), polling);
    assert_eq!(super::habits::findings(root, Some("tests/polling.rs")).unwrap().len(), 3);
}
