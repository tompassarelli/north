#[test]
fn a_failed_nextest_case_is_named_by_its_crate_and_case() {
    let log = "2026-11-13T09:00:00.000Z         FAIL [   0.049s] (3/63) north-capacity-host capacity::host::landing_tests::a_land_check\n        FAIL [   1.200s] north::bin/north cli::parses\n        PASS [   0.010s] (1/63) north-landing land::ok\n";
    let cases: Vec<(String, String)> = super::failed_cases(log).into_iter().collect();
    assert_eq!(cases, [("north".to_owned(), "cli::parses".to_owned()), ("north-capacity-host".to_owned(), "capacity::host::landing_tests::a_land_check".to_owned())]);
}
