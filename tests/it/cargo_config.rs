#[test]
fn dev_and_test_builds_run_rustc_without_the_parallel_front_end() {
    let config = std::fs::read_to_string(crate::support::manifest_dir().join(".cargo/config.toml")).unwrap();
    let threaded: Vec<&str> = config.lines().filter(|line| line.contains("-Zthreads")).collect();
    assert!(threaded.is_empty(), "the workspace cargo config turns on rustc's parallel front end, which deadlocks intermittently: {threaded:?}");
}
