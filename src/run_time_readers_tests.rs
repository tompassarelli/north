use super::RUN_TIME_READERS;

#[test]
fn the_defaults_reader_accepts_a_route_the_router_can_serve() {
    run_time_data_install();
    let entry = "[codex_route_newcat]\nvalue = 'claude'\nallowed = { one_of = ['codex', 'claude'] }\n";
    let findings = north_base::run_time_data::check("harness/defaults.toml", None, Some(entry));
    assert!(findings.iter().all(|finding| !finding.reason.contains("codex_route_newcat")), "{findings:?}");

    let seat = |provider: &str| {
        format!(
            "[coordinator_chain]\nvalue = [{{ provider = '{provider}', account = 'coordinator@example.com', model = 'sonnet' }}]\nallowed = {{ min_len = 0 }}\n\
             [public_only_providers]\nvalue = ['gemini']\nallowed = {{ min_len = 0 }}\n"
        )
    };
    let private = north_base::run_time_data::check("harness/defaults.toml", None, Some(&seat("claude")));
    assert!(private.iter().all(|finding| !finding.reason.contains("public-only")), "a seat on a provider that may read private repositories is accepted: {private:?}");
    let public = north_base::run_time_data::check("harness/defaults.toml", None, Some(&seat("gemini")));
    assert!(public.iter().any(|finding| finding.reason.contains("which is public-only")), "a seat on a public-only provider is refused before it lands: {public:?}");
}

fn run_time_data_install() {
    north_base::run_time_data::install(&RUN_TIME_READERS);
}
