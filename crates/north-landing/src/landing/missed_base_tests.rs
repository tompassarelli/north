use north_land_checks::landing::missed::classify;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_failure_that_also_fails_on_the_base_is_an_environment_failure_not_a_miss() {
    let outside = |_: &str| false;
    let steady = |_: &str| false;
    let base = names(&["sandbox::confines"]);
    let old = classify(&names(&["sandbox::confines"]), Some(&base), steady, outside);
    assert!(old.left_out.is_empty(), "{old:?}");
    assert_eq!(old.environment, names(&["sandbox::confines"]));
    let new = classify(&names(&["fresh::regression"]), Some(&base), steady, outside);
    assert_eq!(new.left_out, names(&["fresh::regression"]));
    assert!(new.environment.is_empty());
    let unknown = classify(&names(&["sandbox::confines"]), None, steady, outside);
    assert_eq!(unknown.left_out, names(&["sandbox::confines"]), "no base run to compare leaves it a miss");
}

#[test]
fn a_failure_no_change_reached_is_listed_as_flaky_and_never_as_a_selection_gap() {
    let failed = names(&["carried", "flaky", "held", "gap"]);
    let base = names(&["carried"]);
    let verdict = classify(&failed, Some(&base), |name| name == "flaky", |name| name == "held");
    assert_eq!((verdict.environment, verdict.flaky, verdict.selected, verdict.left_out), (names(&["carried"]), names(&["flaky"]), names(&["held"]), names(&["gap"])));
}
