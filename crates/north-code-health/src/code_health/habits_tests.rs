use super::habits::findings;
use std::path::Path;

fn call(prefix: &str, name: &str, args: &str, tail: &str) -> String {
    format!("{prefix}{name}{args}{tail}")
}

#[test]
fn the_north_tree_keeps_its_rust_habits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = findings(&root, None).unwrap();
    assert!(found.is_empty(), "{}", found.join("\n"));
    a_test_reading_the_host_outside_the_fixture_beyond_its_listed_count_is_named_and_a_stale_count_is_refused();
    a_test_reading_time_or_sleeping_outside_the_fixture_is_named_at_each_site();
    a_test_starting_a_fleet_session_or_lander_outside_a_fixture_root_is_named_at_each_site();
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

fn a_test_reading_time_or_sleeping_outside_the_fixture_is_named_at_each_site() {
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
    let racy = format!(
        "use std::thread::sleep;\n#[test]\nfn t() {{\n    {};\n    {};\n    {};\n    SystemTime :: now();\n    {};\n    {};\n}}\n",
        call("std::time::Instant", "::now", "()", ""),
        call("std::thread::sleep", "", "(d)", ""),
        call("tokio::time::sleep", "", "(d)", ".await"),
        call("clock.sleep", "", "(d)", ""),
        call("run(\"sleep 1; Instant", "::now", "()", "\")"),
    );
    let sources = [
        ("tests/racy.rs", racy.clone()),
        ("tests/clock.rs", racy),
        ("tests/listed.rs", format!("#[test]\nfn t() {{\n    let start = {};\n}}\n", call("Instant", "::now", "()", ""))),
        ("src/live.rs", format!("fn live() {{\n    {};\n}}\n#[cfg(test)]\nmod tests {{\n    fn t() {{\n        let _ = std::time::Instant{}now;\n    }}\n}}\n", call("std::thread::sleep", "", "(d)", ""), "::")),
    ]
    .map(|(path, text)| (path.to_owned(), text))
    .to_vec();
    let files = files_of(&habits, sources);
    let found: Vec<String> = check(&habits, &files, None).iter().map(|finding| format!("{}:{} {}", finding.path, finding.line, finding.item)).collect();
    let expected = [
        "tests/racy.rs:1 thread::sleep".to_owned(),
        "tests/racy.rs:4 Instant::now".to_owned(),
        "tests/racy.rs:5 thread::sleep".to_owned(),
        "tests/racy.rs:6 time::sleep".to_owned(),
        format!("tests/racy.rs:7 {}", ["System", "Time", "::", "now"].concat()),
        "src/live.rs:7 Instant::now".to_owned(),
    ];
    assert_eq!(found, expected);
}

fn a_test_starting_a_fleet_session_or_lander_outside_a_fixture_root_is_named_at_each_site() {
    use super::habits::{Habits, check, files_of};
    let config = r#"
[rust_habits]
checks = ["ambient-test-input"]
[[rust_habits.ambient_inputs]]
input = "fleet"
allowlist = "fleets.tsv"
fixtures = ["tests/fixture.rs"]
fixture = "start it under NORTH_TEST_ROOT"
"#;
    let mut habits = Habits::parse(config).unwrap().unwrap();
    habits.read_ambient(|_| Ok("tests/shrunk.rs\t2\n".to_owned())).unwrap();
    let starts = "#[test]\nfn t() {\n    run(&[\"fleet\", \"launch\", \"--name\", \"x\"]);\n    run(&[\"land\", \"serve\"]);\n}\n";
    let sources = [
        ("tests/new.rs", starts.to_owned()),
        ("tests/shrunk.rs", "#[test]\nfn t() {\n    run(&[\"land\", \"serve\"]);\n}\n".to_owned()),
        ("tests/contained.rs", "#[test]\nfn t() {\n    command.env(\"NORTH_TEST_ROOT\", root);\n    run(&[\"land\", \"serve\"]);\n}\n".to_owned()),
        ("tests/fixture.rs", starts.to_owned()),
        ("src/live.rs", "fn live() {\n    run(&[\"fleet\", \"launch\"]);\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        run(&[\"land\", \"serve\"]);\n    }\n}\n".to_owned()),
    ]
    .map(|(path, text)| (path.to_owned(), text))
    .to_vec();
    let files = files_of(&habits, sources);
    let found: Vec<String> = check(&habits, &files, None).iter().map(|finding| format!("{}:{}", finding.path, finding.line)).collect();
    assert_eq!(found, ["tests/new.rs:3", "tests/new.rs:4", "src/live.rs:7", "tests/shrunk.rs:0"]);
}

#[test]
fn an_aged_time_in_test_code_is_suggested_as_a_fixed_moment_and_a_poll_loop_is_left_for_the_agent() {
    use super::habits::suggestions;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(root).args(args).status().unwrap().success());
    git(&["init", "-q"]);
    std::fs::write(root.join("structure.toml"), "[rust_habits]\nchecks = [\"ambient-test-input\"]\n[[rust_habits.ambient_inputs]]\ninput = \"clock\"\nallowlist = \"clocks.tsv\"\nfixture = \"inject the clock\"\n").unwrap();
    std::fs::write(root.join("clocks.tsv"), "").unwrap();
    std::fs::create_dir(root.join("tests")).unwrap();
    let system_time = ["std::time::", "System", "Time", "::now"].concat();
    let instant = ["Instant", "::now()"].concat();
    let sleep = ["std::thread::", "sleep", "(READY_POLL);"].concat();
    let denylist = format!("#[test]\nfn stale() {{\n    let hour_ago = {system_time}() - std::time::Duration::from_secs(3600);\n    set(hour_ago);\n}}\n");
    let polling = format!("#[test]\nfn ready() {{\n    let started = {instant};\n    {sleep}\n    let _ = {system_time}();\n}}\n");
    std::fs::write(root.join("tests/denylist.rs"), &denylist).unwrap();
    std::fs::write(root.join("tests/polling.rs"), &polling).unwrap();

    let suggested = suggestions(root, "tests/denylist.rs").unwrap();
    assert_eq!(suggested.len(), 1);
    assert_eq!(suggested[0].rule, "ambient-test-input");
    assert_eq!(suggested[0].line, 3);
    assert_eq!(suggested[0].before, format!("let hour_ago = {system_time}() - std::time::Duration::from_secs(3600);"));
    assert_eq!(suggested[0].after, "let hour_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000) - std::time::Duration::from_secs(3600);");
    assert_eq!(std::fs::read_to_string(root.join("tests/denylist.rs")).unwrap(), denylist, "a suggestion must leave the agent's read intact");
    assert_eq!(super::habits::findings(root, Some("tests/denylist.rs")).unwrap().len(), 1);

    assert!(suggestions(root, "tests/polling.rs").unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("tests/polling.rs")).unwrap(), polling);
    assert_eq!(super::habits::findings(root, Some("tests/polling.rs")).unwrap().len(), 3);
}

#[test]
fn an_allowlist_count_raised_above_the_base_revision_is_refused_and_a_lowered_one_is_not() {
    use super::habits::{findings_new, grown};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(root).args(args).status().unwrap().success());
    git(&["init", "-q"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("structure.toml"), "[rust_habits]\nchecks = [\"ambient-test-input\"]\n[[rust_habits.ambient_inputs]]\ninput = \"clock\"\nallowlist = \"clocks.tsv\"\nfixture = \"inject the clock\"\n").unwrap();
    std::fs::write(root.join("clocks.tsv"), "tests/kept.rs\t1\n").unwrap();
    std::fs::create_dir(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/debt.rs"), format!("#[test]\nfn t() {{\n    {};\n}}\n", call("std::thread::sleep", "", "(d)", ""))).unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);

    let base_findings = findings_new(root, "HEAD").unwrap();
    assert!(base_findings.is_empty(), "a finding the base already holds is not the lane's: {base_findings:?}");
    std::fs::write(root.join("tests/debt.rs"), format!("#[test]\nfn t() {{\n    {};\n    {};\n}}\n", call("std::thread::sleep", "", "(d)", ""), call("std::thread::sleep", "", "(d)", ""))).unwrap();
    let added = findings_new(root, "HEAD").unwrap();
    assert_eq!(added.len(), 1, "a finding the lane adds beside an old one is new: {added:?}");
    assert!(added[0].contains("tests/debt.rs:4"), "{added:?}");
    std::fs::write(root.join("tests/debt.rs"), format!("#[test]\nfn t() {{\n    {};\n}}\n", call("std::thread::sleep", "", "(d)", ""))).unwrap();

    std::fs::write(root.join("clocks.tsv"), "tests/kept.rs\t2\ntests/new.rs\t1\n").unwrap();
    std::fs::write(root.join("tests/new.rs"), format!("#[test]\nfn t() {{\n    {};\n}}\n", call("std::thread::sleep", "", "(d)", ""))).unwrap();
    let raised = grown(root, "HEAD").unwrap();
    assert_eq!(raised.len(), 2, "{raised:?}");
    assert!(raised.iter().any(|line| line.contains("tests/kept.rs") && line.contains("up from 1")), "{raised:?}");
    assert!(raised.iter().any(|line| line.contains("tests/new.rs") && line.contains("up from 0")), "{raised:?}");
    assert_eq!(findings_new(root, "HEAD").unwrap().len(), 2, "a lane that raises an allowlist count meets the grown line");

    std::fs::write(root.join("clocks.tsv"), "tests/kept.rs\t0\n").unwrap();
    assert!(grown(root, "HEAD").unwrap().is_empty());
}
