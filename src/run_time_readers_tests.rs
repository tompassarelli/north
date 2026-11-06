use super::RUN_TIME_READERS;

#[test]
fn the_defaults_reader_refuses_a_route_the_router_would_panic_on() {
    run_time_data_install();
    let entry = "[codex_route_newcat]\nvalue = 'gpt'\nallowed = { prefix = 'g' }\n";
    let findings = north_core::run_time_data::check("harness/defaults.toml", None, Some(entry));
    assert!(findings.iter().any(|finding| finding.reason.contains("codex_route_newcat: unknown value gpt")), "{findings:?}");
}

fn run_time_data_install() {
    north_core::run_time_data::install(&RUN_TIME_READERS);
}
