use super::*;

const NOW: u64 = 1_800_000_000;

#[test]
fn a_main_run_that_failed_only_in_its_environment_steps_warns_and_does_not_page_a_red_main() {
    let run = |environment_only| Run { name: "Canary".into(), head_branch: "main".into(), event: "push".into(), status: "completed".into(), conclusion: Some("failure".into()), head_sha: "5b284a8f28ac".into(), environment_only, ..Run::default() };
    let alerts = |environment_only| {
        let context = WatchContext { now: NOW, repos: vec![RepoFacts { name: "north".into(), runs: vec![run(environment_only)], ..RepoFacts::default() }], ..WatchContext::default() };
        red_main(&context, &context.repos[0])
    };
    assert_eq!(alerts(true).len(), 1);
    assert_eq!(alerts(true)[0].severity, Severity::Warn);
    assert!(alerts(true)[0].summary.contains("environment not set up"));
    assert_eq!(alerts(false)[0].severity, Severity::Page);
    let jobs = r#"{"jobs":[{"steps":[{"name":"Environment: cargo tests","conclusion":"failure"},{"name":"Test","conclusion":"skipped"}]}]}"#;
    assert!(super::super::facts::failed_only_in_environment_steps(jobs));
    assert!(!super::super::facts::failed_only_in_environment_steps(&jobs.replace("skipped", "failure")));
    assert!(super::super::facts::started_no_job(r#"{"total_count":0,"jobs":[]}"#));
    assert!(!super::super::facts::started_no_job(jobs));
    let jobless = WatchContext { now: NOW, repos: vec![RepoFacts { name: "north".into(), runs: vec![Run { started_no_job: true, ..run(false) }], ..RepoFacts::default() }], ..WatchContext::default() };
    assert!(red_main(&jobless, &jobless.repos[0]).is_empty());
    let unjudged = WatchContext { now: NOW, repos: vec![RepoFacts { name: "north".into(), runs: vec![Run { unjudged: true, ..run(false) }], ..RepoFacts::default() }], ..WatchContext::default() };
    assert!(red_main(&unjudged, &unjudged.repos[0]).is_empty(), "a red run the runtime recorded as not judged, such as hosted run 43637764867 whose cases only timed out, does not page main red");
    let records = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(records.path().join("buck2-after-landing")).unwrap();
    std::fs::write(records.path().join("buck2-after-landing/e619fbd6fa.unjudged"), "run\t43637764867\nwhy\tits cases and targets only timed out, none failed\n").unwrap();
    let timings = north_landing_gates::landing::check_timings::Timings::at(records.path().to_owned());
    assert!(timings.buck2_unjudged("e619fbd6fa", 43637764867) && !timings.buck2_unjudged("e619fbd6fa", 1) && !timings.buck2_unjudged("other", 43637764867));
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(state.path().join("north/land-queue/q")).unwrap();
    std::fs::write(state.path().join("north/land-queue/q/lander-post-land.tsv"), "1794000000\tnorth\t577e63c682e6aaaa\tgreen\tpassed\n").unwrap();
    let hosted = |sha: &str| Run { name: "Cargo".into(), head_branch: "main".into(), event: "push".into(), status: "completed".into(), conclusion: Some("failure".into()), head_sha: sha.into(), ..Run::default() };
    let alerts = |sha: &str| {
        let context = WatchContext { now: NOW, state: state.path().to_owned(), repos: vec![RepoFacts { name: "north".into(), runs: vec![hosted(sha)], ..RepoFacts::default() }], ..WatchContext::default() };
        red_main(&context, &context.repos[0])
    };
    assert_eq!((alerts("577e63c682e6aaaa")[0].severity, alerts("577e63c682e6aaaa").len()), (Severity::Warn, 1));
    assert_eq!(alerts("0123456789ab")[0].severity, Severity::Page);
}
