use super::habits::findings;
use std::path::Path;

#[test]
fn the_north_tree_keeps_its_rust_habits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = findings(&root, None).unwrap();
    assert!(found.is_empty(), "{}", found.join("\n"));
    a_test_reading_the_host_outside_the_fixture_beyond_its_listed_count_is_named_and_a_stale_count_is_refused();
}

fn a_test_reading_the_host_outside_the_fixture_beyond_its_listed_count_is_named_and_a_stale_count_is_refused() {
    use super::habits::{Habits, check, files_of};
    let config = r#"
[rust_habits]
checks = ["ambient-test-input"]
[rust_habits.ambient_inputs]
allowlist = "list.tsv"
fixtures = ["tests/fixture.rs"]
fixture = "pin the reading"
"#;
    let mut habits = Habits::parse(config).unwrap().unwrap();
    habits.read_ambient("tests/listed.rs\t1\ntests/shrunk.rs\t2\n").unwrap();
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
