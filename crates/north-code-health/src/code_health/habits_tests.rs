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
