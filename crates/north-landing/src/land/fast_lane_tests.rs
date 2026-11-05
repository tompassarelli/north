use super::*;

fn workspace() -> Vec<Package> {
    [("north", "", true), ("north-core", "crates/north-core/", false), ("north-landing", "crates/north-landing/", false), ("north-tool", "crates/north-tool/", true)]
        .map(|(name, directory, leaf)| Package { name: name.to_owned(), directory: directory.to_owned(), leaf })
        .to_vec()
}

fn routed(changed: &[&str], queued: &[&str]) -> Route {
    let changed: BTreeSet<String> = changed.iter().map(|path| (*path).to_owned()).collect();
    let queued: HashSet<String> = queued.iter().map(|path| (*path).to_owned()).collect();
    route(&Facts { changed: &changed, queued: &queued, packages: &workspace(), red_fix: false })
}

fn full(route: &Route) -> bool {
    matches!(route, Route::Full(_))
}

#[test]
fn docs_tests_and_one_leaf_crate_take_the_fast_lane_with_only_their_crates_checked() {
    assert_eq!(routed(&["docs/rationale/x.md", "README.md"], &[]), Route::Fast(Vec::new()));
    assert_eq!(routed(&["crates/north-core/src/a_tests.rs", "crates/north-core/tests/b.rs"], &[]), Route::Fast(vec!["north-core".to_owned()]));
    assert_eq!(routed(&["src/cli/x.rs", "tests/it/x.rs", "docs/x.md"], &[]), Route::Fast(vec!["north".to_owned()]));
    assert_eq!(routed(&["crates/north-tool/src/lib.rs"], &[]), Route::Fast(vec!["north-tool".to_owned()]));
}

#[test]
fn overlap_lander_code_shared_crates_several_crates_build_files_and_red_fixes_take_the_full_precheck() {
    assert!(full(&routed(&["docs/x.md"], &["docs/x.md"])));
    assert!(full(&routed(&["crates/north-landing/src/land/x_tests.rs"], &[])));
    assert!(full(&routed(&["crates/north-core/src/lib.rs"], &[])));
    assert!(full(&routed(&["src/x.rs", "crates/north-tool/src/lib.rs"], &[])));
    assert!(full(&routed(&["Cargo.lock"], &[])));
    assert!(full(&routed(&["crates/north-tool/Cargo.toml"], &[])));
    let changed: BTreeSet<String> = ["docs/x.md".to_owned()].into();
    assert!(full(&route(&Facts { changed: &changed, queued: &HashSet::new(), packages: &workspace(), red_fix: true })));
}

#[test]
fn packages_reads_members_and_marks_a_crate_nothing_depends_on_as_a_leaf() {
    let root = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        let file = root.path().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    };
    write("Cargo.toml", "[workspace]\nmembers = [\"crates/base\", \"crates/tool\"]\n\n[package]\nname = \"app\"\n\n[dependencies]\nbase = { path = \"crates/base\" }\n");
    write("crates/base/Cargo.toml", "[package]\nname = \"base\"\n");
    write("crates/tool/Cargo.toml", "[package]\nname = \"tool\"\n\n[dependencies]\nbase.workspace = true\n");
    let mut found = packages(root.path());
    found.sort_by(|left, right| left.name.cmp(&right.name));
    let summary: Vec<(&str, &str, bool)> = found.iter().map(|package| (package.name.as_str(), package.directory.as_str(), package.leaf)).collect();
    assert_eq!(summary, [("app", "", true), ("base", "crates/base/", false), ("tool", "crates/tool/", true)]);
}
