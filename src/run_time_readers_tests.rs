use super::RUN_TIME_READERS;

#[test]
fn the_defaults_reader_accepts_a_route_the_router_can_serve() {
    run_time_data_install();
    let entry = "[codex_route_newcat]\nvalue = 'claude'\nallowed = { one_of = ['codex', 'claude'] }\n";
    let findings = north_base::run_time_data::check("harness/defaults.toml", None, Some(entry));
    assert!(findings.iter().all(|finding| !finding.reason.contains("codex_route_newcat")), "{findings:?}");
}

fn run_time_data_install() {
    north_base::run_time_data::install(&RUN_TIME_READERS);
}
