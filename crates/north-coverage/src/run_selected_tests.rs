use super::*;

fn test(binary: &str, name: &str) -> Test {
    Test { binary: binary.to_owned(), name: name.to_owned() }
}

#[test]
fn selected_tests_build_only_their_packages_and_targets_and_run_by_exact_name() {
    let tests: BTreeSet<Test> = [test("north-base", "a::b"), test("north::it", "land_checks::c"), test("north::bin/north", "d")].into();
    assert_eq!(
        nextest_arguments(&tests),
        vec![
            "-p north".to_owned(),
            "-p north-base".to_owned(),
            "--bin north".to_owned(),
            "--lib".to_owned(),
            "--test it".to_owned(),
            "-E '(binary_id(=north-base) & test(=a::b)) | (binary_id(=north::bin/north) & test(=d)) | (binary_id(=north::it) & test(=land_checks::c))'".to_owned(),
        ]
    );
}

#[test]
fn a_request_takes_the_two_revisions_and_its_flags_and_refuses_anything_else() {
    let words = |text: &str| text.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    let parsed = request(&words("main HEAD --map /m.json --timeout 60")).unwrap();
    assert_eq!((parsed.base.as_str(), parsed.head.as_str(), parsed.map, parsed.timeout.as_str()), ("main", "HEAD", Some(PathBuf::from("/m.json")), "60"));
    assert!(request(&words("main")).is_none());
    assert!(request(&words("main HEAD --timeout soon")).is_none());
}
