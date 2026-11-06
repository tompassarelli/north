// KEEP(bug): on 2026-11-06 rustc's parallel front end (-Zthreads=8) deadlocked on gex45 and ccx33, its threads parked on futexes and on cargo's jobserver pipe at 0 to 4 CPU seconds for 13 to 23 minutes, so promotion replays of the coverage check hung past their deadline and passed on rerun (north#634)
#[test]
fn dev_and_test_builds_run_rustc_without_the_parallel_front_end() {
    let config = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/.cargo/config.toml")).unwrap();
    let threaded: Vec<&str> = config.lines().filter(|line| line.contains("-Zthreads")).collect();
    assert!(threaded.is_empty(), "the workspace cargo config turns on rustc's parallel front end, which deadlocks intermittently: {threaded:?}");
}
