use super::*;

#[test]
fn a_budget_line_is_a_workflow_seconds_and_an_optional_ceiling_above_them() {
    assert!(Budget::parse("cargo.yml 162 600").is_some_and(|budget| budget.ceiling == Some(600.0)));
    assert!(Budget::parse("cargo.yml 162.5").is_some_and(|budget| budget.seconds == 162.5 && budget.ceiling.is_none()));
    for invalid in ["cargo.yml", "cargo.yml 1e3", "cargo.yml 162 100", "cargo.yml 162 600 extra", "cargo.yml -1", "cargo.yml 1."] {
        assert!(Budget::parse(invalid).is_none(), "{invalid}");
    }
}

#[test]
fn a_speed_up_trailer_names_the_workflow_and_a_smaller_after() {
    assert!(speed_up("farm-test.yml 300s -> 200s", "farm-test.yml"));
    assert!(!speed_up("farm-test.yml 200s -> 300s", "farm-test.yml"));
    assert!(!speed_up("other.yml 300s -> 200s", "farm-test.yml"));
    assert!(!speed_up("farm-test.yml 300 -> 200", "farm-test.yml"));
    assert!(!speed_up("farm-test.yml 300s -> 200s more", "farm-test.yml"));
}

#[test]
fn a_job_list_measures_wall_time_and_the_slowest_job() {
    let jobs = r#"[{"jobs":[{"name":"bootstrap","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:02:00Z"}]},
        {"jobs":[{"name":"integration","started_at":"2026-10-11T00:03:20Z","completed_at":"2026-10-11T00:08:00Z"},{"name":"queued","started_at":null,"completed_at":null}]}]"#;
    let measurement = Measurement::of_jobs(jobs).flatten().expect("measured");
    assert_eq!((measurement.wall, measurement.slowest.as_str(), measurement.slowest_seconds), (480, "integration", 280));
    assert!(Measurement::of_jobs(r#"[{"jobs":[]}]"#).is_some_and(|measured| measured.is_none()));
    assert!(Measurement::of_jobs("not json").is_none());
}
