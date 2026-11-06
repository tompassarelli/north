use super::*;

fn test(binary: &str, name: &str) -> Test {
    Test { binary: binary.to_owned(), name: name.to_owned() }
}

#[test]
fn selected_tests_build_only_their_packages_and_targets_and_run_by_exact_name() {
    let tests: BTreeSet<Test> = [test("north-base", "a::b"), test("north::it", "land_checks::c"), test("north::bin/north", "d")].into();
    assert_eq!(
        nextest_arguments(&tests, &[]),
        vec![
            "-p north".to_owned(),
            "-p north-base".to_owned(),
            "--bin north".to_owned(),
            "--lib".to_owned(),
            "--test it".to_owned(),
            "-E '(binary_id(=north-base) & test(=a::b)) | (binary_id(=north::bin/north) & test(=d)) | (binary_id(=north::it) & test(=land_checks::c))'".to_owned(),
        ]
    );
    let added = [Added { package: "north-base".to_owned(), target: "--lib".to_owned(), name: "brand_new".to_owned() }];
    assert_eq!(nextest_arguments(&BTreeSet::new(), &added), vec!["-p north-base".to_owned(), "--lib".to_owned(), "-E '(package(=north-base) & test(~brand_new))'".to_owned()]);
}


