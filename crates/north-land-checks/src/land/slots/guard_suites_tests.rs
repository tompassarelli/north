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
    let privacy = suites.floor(&["crates/north-timers/src/privacy/judge.rs".to_owned(), "README.md".to_owned()]);
    assert_eq!(privacy.names(), "privacy gate");
    let graph = Tests::parse(r#"{"tests":["north-ops ops::report::tests::reads"],"known":1}"#).unwrap();
    let selection = Selection::Tests(graph).with_floor(privacy);
    let arguments = selection.arguments();
    for term in ["(package(north-timers)&test(/^privacy::/))", "(package(north-base)&test(/^platform::private_text::/))", "(package(north-landing-gates)&test(/^landing::denylist/))", "(binary_id(north-ops)&(test(=ops::report::tests::reads)))"] {
        assert!(arguments.contains(term), "a privacy-judge lane runs {term}: {arguments}");
    }
    assert!(selection.packages().is_superset(&["north-timers", "north-base", "north-landing-gates", "north-land-scans", "north-safe-push"].map(str::to_owned).into()), "the suite's packages are built");
    let sandbox = Selection::Modules { modules: BTreeSet::new(), units: Default::default(), floor: Floor::default() }.with_floor(suites.floor(&["crates/north-land-sandbox/src/lib.rs".to_owned()]));
    assert!(sandbox.runs_module("worker_sandbox") && sandbox.packages().contains("north-land-sandbox"), "the crate fallback keeps the floor too: {}", sandbox.summary());
    let landing = suites.floor(&["crates/north-landing/src/land/post_land/execute.rs".to_owned()]);
    assert_eq!(landing.names(), "landing machinery");
    assert!(landing.selects("land_chaos::chaos_lander_stopped_by_sigterm_mid_after_run_adopts_its_live_runner_and_reverts_the_red_lane"), "the landing machinery floor holds the chaos test");
    assert!(!suites.floor(&["crates/north-ops/src/lib.rs".to_owned()]).selects("land_chaos::chaos_lander_stopped_by_sigterm_mid_after_run_adopts_its_live_runner_and_reverts_the_red_lane"), "an unguarded path holds nothing");
    let floor = Selection::Tests(Tests::parse(r#"{"tests":["north-ops ops::report::tests::reads"],"known":1}"#).unwrap()).with_floor(landing);
    assert!(floor.packages().is_superset(&["north-lander", "north-land-checks", "north-safe-push"].map(str::to_owned).into()), "a north-landing lane builds the landing machinery tests, including north-lander's chaos test");
    assert!(floor.arguments().contains("package(north-lander)"), "the north-lander unit tests run for a post-land runner change: {}", floor.arguments());
    assert!(floor.arguments().starts_with("--tests "), "every test target is built, so the case runs in north-lander's integration binary: {}", floor.arguments());
    assert!(floor.arguments().contains("(binary_id(north-lander::it)&test(=land_chaos::chaos_lander_stopped_by_sigterm_mid_after_run_adopts_its_live_runner_and_reverts_the_red_lane))"), "the chaos test that guards the post-land runner runs: {}", floor.arguments());
    let fallback = Selection::Modules { modules: BTreeSet::new(), units: Default::default(), floor: suites.floor(&["crates/north-landing/src/land/post_land/execute.rs".to_owned()]) };
    assert!(fallback.arguments().contains("(binary_id(north-lander::it)&test(=land_chaos::chaos_lander_stopped_by_sigterm_mid_after_run_adopts_its_live_runner_and_reverts_the_red_lane))"), "the crate fallback keeps the guard case too: {}", fallback.arguments());
    assert!(suites.floor(&["crates/north-ops/src/lib.rs".to_owned()]).is_empty(), "an unguarded path runs no suite");

    let landed = suites.floor(&[".safe-push".to_owned()]);
    assert_eq!(landed.names(), "landing machinery");
    assert_eq!(suites.floor(&["harness/guard-suites.toml".to_owned()]).names(), "landing machinery", "a change to the table itself runs the tests that read it");
    assert!(landed.selects("land_checks::north_land_check_refuses_a_lane_that_adds_a_reference_its_module_layers_forbid"), "a .safe-push lane holds the land_checks tests it embeds, so north land missed does not read the a84a8a9ca miss as left out");
    let embedded = Selection::Tests(Tests::parse(r#"{"tests":["north-ops ops::report::tests::reads"],"known":1}"#).unwrap()).with_floor(landed);
    assert!(embedded.arguments().contains("(binary_id(north::it)&test(/^(land_checks)::/))"), "a lane changing only .safe-push runs the land_checks it module: {}", embedded.arguments());

    let renamed = |from: &str, to: Option<&str>| vec![(from.to_owned(), to.map(str::to_owned))];
    assert!(suites.moved_out(&renamed("crates/north-runtime/src/worker_sandbox/readable.rs", Some("crates/north-runtime/src/worker_sandbox/paths/readable.rs")), &[]).is_empty(), "a move inside the suite's paths passes");
    assert_eq!(suites.moved_out(&renamed("crates/north-timers/src/privacy/judge.rs", None), &["crates/north-core/src/judge.rs".to_owned()]).len(), 1, "deleting a guarded file while adding Rust outside the suite is refused");
    assert!(suites.moved_out(&renamed("crates/north-timers/src/privacy/judge.rs", None), &["docs/privacy.md".to_owned()]).is_empty(), "a deletion that adds no Rust elsewhere passes");

    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("harness")).unwrap();
    std::fs::write(directory.path().join(TABLE), "[[suite]]\nname = \"gone\"\npaths = [\"crates/gone/\"]\ntests = [\"gone\"]\nit = []\n").unwrap();
    let mut out = Vec::new();
    assert!(refuse(directory.path(), "HEAD", "HEAD", &mut out), "a guarded path missing at the lane's head refuses it");
    assert!(String::from_utf8(out).unwrap().contains("crates/gone/"));
    assert!(!refuse(tempfile::tempdir().unwrap().path(), "HEAD", "HEAD", &mut Vec::new()), "a repository without the table has no floor");
}
