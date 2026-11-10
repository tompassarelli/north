use super::*;
use super::super::graph_selection::Tests;
use super::super::it_modules::Selection;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn suites() -> Suites {
    Suites::read(root()).unwrap()
}

#[test]
fn a_lane_touching_a_guarded_path_runs_its_guard_suite() {
    let suites = suites();
    assert_eq!(suites.missing(root()), Vec::<String>::new(), "every path {TABLE} guards exists");
    let privacy = suites.floor(&["crates/north-runtime/src/privacy/judge.rs".to_owned(), "README.md".to_owned()]);
    assert_eq!(privacy.names(), "privacy gate");
    let graph = Tests::parse(r#"{"tests":["north-ops ops::report::tests::reads"],"known":1}"#).unwrap();
    let selection = Selection::Tests(graph).with_floor(privacy);
    let arguments = selection.arguments();
    for term in ["(package(north-runtime)&test(/^privacy::/))", "(package(north-base)&test(/^platform::private_text::/))", "(package(north-landing-gates)&test(/^landing::denylist/))", "(binary_id(north-ops)&(test(=ops::report::tests::reads)))"] {
        assert!(arguments.contains(term), "a privacy-judge lane runs {term}: {arguments}");
    }
    assert!(selection.packages().is_superset(&["north-runtime", "north-base", "north-landing-gates", "north-land-scans", "north-safe-push"].map(str::to_owned).into()), "the suite's packages are built");
    let sandbox = Selection::Modules { modules: BTreeSet::new(), units: Default::default(), floor: Floor::default() }.with_floor(suites.floor(&["crates/north-land-sandbox/src/lib.rs".to_owned()]));
    assert!(sandbox.runs_module("worker_sandbox") && sandbox.packages().contains("north-land-sandbox"), "the crate fallback keeps the floor too: {}", sandbox.summary());
    assert!(suites.floor(&["crates/north-ops/src/lib.rs".to_owned()]).is_empty(), "an unguarded path runs no suite");

    let renamed = |from: &str, to: Option<&str>| vec![(from.to_owned(), to.map(str::to_owned))];
    assert!(suites.moved_out(&renamed("crates/north-runtime/src/worker_sandbox/readable.rs", Some("crates/north-runtime/src/worker_sandbox/paths/readable.rs")), &[]).is_empty(), "a move inside the suite's paths passes");
    assert_eq!(suites.moved_out(&renamed("crates/north-runtime/src/privacy/judge.rs", None), &["crates/north-core/src/judge.rs".to_owned()]).len(), 1, "deleting a guarded file while adding Rust outside the suite is refused");
    assert!(suites.moved_out(&renamed("crates/north-runtime/src/privacy/judge.rs", None), &["docs/privacy.md".to_owned()]).is_empty(), "a deletion that adds no Rust elsewhere passes");

    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("harness")).unwrap();
    std::fs::write(directory.path().join(TABLE), "[[suite]]\nname = \"gone\"\npaths = [\"crates/gone/\"]\ntests = [\"gone\"]\nit = []\n").unwrap();
    let mut out = Vec::new();
    assert!(refuse(directory.path(), "HEAD", "HEAD", &mut out), "a guarded path missing at the lane's head refuses it");
    assert!(String::from_utf8(out).unwrap().contains("crates/gone/"));
    assert!(!refuse(tempfile::tempdir().unwrap().path(), "HEAD", "HEAD", &mut Vec::new()), "a repository without the table has no floor");
}
