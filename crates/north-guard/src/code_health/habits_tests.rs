use super::habits::findings;
use std::path::Path;

#[test]
fn the_north_tree_keeps_its_rust_habits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = findings(&root, None).unwrap();
    assert!(found.is_empty(), "{}", found.join("\n"));
    a_test_reading_the_clock_or_the_host_beyond_its_listed_count_is_named_and_a_stale_count_is_refused();
}

fn a_test_reading_the_clock_or_the_host_beyond_its_listed_count_is_named_and_a_stale_count_is_refused() {
    use super::habits::{Habits, check, files_of};
    let config = r#"
[rust_habits]
checks = ["ambient-test-input"]
[rust_habits.ambient_inputs]
mode = "refuse"
allowlist = "list.tsv"
fixtures = ["tests/fixture.rs"]
time_fixture = "use the clock fixture"
host_fixture = "pin the reading"
"#;
    let mut habits = Habits::parse(config).unwrap().unwrap();
    habits.read_ambient("tests/listed.rs\t1\ntests/shrunk.rs\t2\n").unwrap();
    let sleeps = "#[test]\nfn t() {\n    std::thread::sleep(d);\n}\n";
    let sources = [
        ("tests/new.rs", sleeps.to_owned()),
        ("tests/listed.rs", sleeps.to_owned()),
        ("tests/shrunk.rs", sleeps.to_owned()),
        ("tests/fixture.rs", sleeps.to_owned()),
        ("tests/paused.rs", "#[tokio::test(start_paused = true)]\nasync fn t() {\n    tokio::time::sleep(d).await;\n}\n".to_owned()),
        ("src/host.rs", "fn live() {\n    Instant::now();\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        std::fs::read(\"/proc/self/stat\");\n    }\n}\n".to_owned()),
    ]
    .map(|(path, text)| (path.to_owned(), text))
    .to_vec();
    let files = files_of(&habits, sources);
    let found: Vec<String> = check(&habits, &files, None).iter().map(|finding| format!("{}:{}", finding.path, finding.line)).collect();
    assert_eq!(found, ["tests/new.rs:3", "src/host.rs:7", "tests/shrunk.rs:0"]);
}
