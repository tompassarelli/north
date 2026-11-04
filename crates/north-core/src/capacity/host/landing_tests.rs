use super::landing_run;
use super::request::Request;

fn admitted(owner: &str, land_head: Option<&str>, landing: Option<&str>) -> String {
    let request = Request { name: "heavy".to_owned(), landing: landing_run("run", owner, "heavy", land_head, landing), ..Request::default() };
    request.admitted_class().to_owned()
}

#[test]
fn a_gated_run_of_an_offloaded_land_check_is_admitted_as_landing() {
    assert_eq!(admitted("gate:session-1", Some("abc"), Some("1")), "critical");
    assert_eq!(admitted("gate:anything", Some("abc"), Some("1")), "critical");
}

#[test]
fn a_gated_run_without_the_landing_marker_stays_plain_heavy() {
    assert_eq!(admitted("gate:session-1", Some("abc"), None), "heavy");
    assert_eq!(admitted("gate:session-1", Some("abc"), Some("")), "heavy");
    assert_eq!(admitted("gate:session-1", Some("abc"), Some("0")), "heavy");
}

#[test]
fn the_landing_marker_alone_or_on_another_owner_does_not_admit_as_landing() {
    assert_eq!(admitted("gate:session-1", None, Some("1")), "heavy");
    assert_eq!(admitted("gate:session-1", Some(""), Some("1")), "heavy");
    assert_eq!(admitted("worker", Some("abc"), Some("1")), "heavy");
}

#[test]
fn the_land_check_owner_is_landing_with_a_land_head() {
    assert_eq!(admitted("land-check", Some("abc"), None), "critical");
    assert_eq!(admitted("land-check", None, None), "heavy");
}

#[test]
fn only_runs_of_batch_classes_are_landing() {
    assert!(!landing_run("session", "land-check", "heavy", Some("abc"), None));
    assert!(!landing_run("run", "land-check", "light", Some("abc"), None));
}
