use super::*;

const NOW: u64 = 1_800_000_000;

fn context(queued_for: u64) -> WatchContext {
    let created = NOW - queued_for;
    let (days, rest) = (created / 86_400, created % 86_400);
    let date = time::Date::from_julian_day(i32::try_from(days).unwrap() + 2_440_588).unwrap();
    let created_at = format!("{date}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60);
    let run = Run { name: "Cargo".into(), head_branch: "main".into(), event: "push".into(), status: "queued".into(), created_at, ..Run::default() };
    WatchContext { now: NOW, repos: vec![RepoFacts { name: "north".into(), runs: vec![run], ..RepoFacts::default() }], ..WatchContext::default() }
}

#[test]
fn a_run_queued_244_s_raises_no_alarm_and_one_past_the_queued_floor_does() {
    assert!(queued(&context(244), &context(244).repos[0]).is_none());
    let stuck = context(700);
    let alert = queued(&stuck, &stuck.repos[0]).expect("a run queued past the floor alarms");
    assert!(alert.summary.contains("queued over 600 s"), "{}", alert.summary);
}

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
}
