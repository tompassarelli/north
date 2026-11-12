use std::path::Path;

use north_dispatch::capacity::host::maximum_seconds;

use super::git_maintenance::maintenance_argv;

#[test]
fn nightly_maintenance_asks_the_capacity_host_for_no_more_than_its_class_maximum() {
    let argv = maintenance_argv("north", Path::new("/home/tom/code/north/main"));
    let value = |option: &str| argv.iter().position(|argument| argument == option).map(|index| argv[index + 1].as_str()).unwrap();
    let timeout: u64 = value("--timeout-seconds").parse().unwrap();
    assert!((1..=maximum_seconds(value("--class"))).contains(&timeout), "--timeout-seconds {timeout} is outside what `north capacity host run` accepts");
}
