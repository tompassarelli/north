use super::*;
use super::super::tree_wide_units;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap()
}

fn selected(files: &[&str]) -> Selection {
    let files: Vec<String> = files.iter().map(|file| (*file).to_owned()).collect();
    select(&read(STRUCTURE), &read(TABLE), &spawners(root()), &files).unwrap()
}

// KEEP(bug): on 2026-11-08 every lander probe whose lanes touched src/ ran the whole tests/it suite (`--test it -E kind(lib)|kind(bin)|binary_id(north::it)`), 6 to 10 min a probe, because a root-package path had no crate_dag owner and fell back to every module (north#842)
#[test]
fn a_probe_through_src_and_four_crates_runs_only_the_tests_it_modules_their_crates_select() {
    let probe = ["src/cli/worker_ledger/recalibrate.rs", "crates/north-calibration/src/threads/calibration/box_eta.rs", "crates/north-capacity-host/src/capacity/command.rs", "crates/north-climb/src/arms.rs", "crates/north-context-pack/src/cochange.rs"];
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let table = Table::parse(&read(TABLE), &plan, &spawners(root())).unwrap();
    let affected = plan.closure(["north", "north-calibration", "north-capacity-host", "north-climb", "north-context-pack"].map(str::to_owned).into());
    let expected: BTreeSet<String> = table.modules().filter(|module| !table.crates(module).unwrap().is_disjoint(&affected)).map(str::to_owned).collect();
    let selection = selected(&probe);
    assert_eq!(selection, Selection::Modules { modules: expected, units: Units::default() }, "the probe runs the modules crates.txt maps to its crates");
    assert!(selection.arguments().starts_with("--test it -E kind(lib)|kind(bin)|(binary_id(north::it)&test(/"), "the root package's unit tests still run");
    let modules = |names: &[&str]| Selection::Modules { modules: names.iter().map(|name| (*name).to_owned()).chain(["affected_modules", "agent_text_reads"].map(str::to_owned)).collect(), units: Units::default() };
    assert_eq!(selected(&["tests/it/threads_ledger.rs"]), modules(&["threads_ledger"]), "an edited module runs with the tree-wide ones");
    assert_eq!(selected(&["tests/it/runtime_update/prune.rs"]), modules(&["runtime_update"]), "a file in a module's directory selects it");
    for file in ["tests/it/support/tree.rs", "tests/it/main.rs", TABLE, "examples/push_trace_git.rs", "Cargo.lock"] {
        assert_eq!(selected(&["src/main.rs", file]), Selection::Every, "{file} runs every module");
    }
}

fn graph_stub(directory: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = directory.join(name);
    north_base::foundation::shim::write_script(&path, &format!("{body}\n"));
    path
}

// KEEP(bug): on 2026-11-08 the crate-level tests/it selection saved 0% of the suite, because 38 of 46 modules spawn the north binary and run for any crate change; the change graph narrows a lane to the test ids it reaches, and an untraced lane keeps the crate-level selection (north#902)
#[test]
fn a_lane_the_graph_traces_runs_only_its_tests_and_an_untraced_lane_runs_the_crate_level_selection() {
    let directory = tempfile::tempdir().unwrap();
    let answer = r#"{"tests":["north::it fleet::launch::refuses_a_full_window","north-land-checks land::slots::select::tests::queues","north-coverage::pick_tests pick::regions"],"always":["north-base::bin/probe tests::reads_home"],"added":[{"package":"north-land-checks","name":"narrows_to_ids"}],"files":[],"untraced":null}"#;
    let traced = graph_stub(directory.path(), "traced", &format!("printf '%s' '{answer}'"));
    let selection = selection_asking(&traced, root(), "HEAD", "HEAD", &Batch::default()).unwrap();
    assert_eq!(
        selection.arguments(),
        "--test it -E (binary_id(north-base::bin/probe)&(test(=tests::reads_home)))|(binary_id(north-land-checks)&(test(=land::slots::select::tests::queues)))|(binary_id(north::it)&(test(=fleet::launch::refuses_a_full_window)))|(package(north-land-checks)&test(/(^|::)narrows_to_ids$/))|(binary_id(north::it)&test(/^(affected_modules|agent_text_reads)::/))|(binary_id(=north-code-health)&test(=code_health::habits_tests::the_north_tree_keeps_its_rust_habits))|(binary_id(=north-runtime)&test(=runtime::reuse::tests::every_file_a_source_embeds_at_compile_time_counts_as_a_rust_input))|(binary_id(=north-runtime)&test(=runtime::reuse::tests::no_source_or_test_embeds_a_table_the_runtime_reads_from_its_tree))",
        "a traced lane runs its traced and always tests in the binaries the land check builds, the tests it adds, and the tree-wide tests/it modules"
    );
    assert!(selection.runs_module("agent_text_reads") && selection.runs_module("fleet") && !selection.runs_module("safe_push"), "a traced lane keeps the tree-wide modules that coverage cannot link to a change (north#706)");
    assert_eq!(selection.packages(), ["north", "north-base", "north-code-health", "north-land-checks", "north-runtime"].map(str::to_owned).into(), "the packages of the selected tests and of the tree-wide unit tests are built");
    let crate_level = select(&read(STRUCTURE), &read(TABLE), &spawners(root()), &[]).unwrap().with_units(units(&read(tree_wide_units::TABLE), &Plan::parse(&read(STRUCTURE)).unwrap()).unwrap());
    for (name, body) in [("untraced", "echo '{\"tests\":[]}'; exit 3"), ("missing", "exit 2")] {
        let program = graph_stub(directory.path(), name, body);
        assert_eq!(selection_asking(&program, root(), "HEAD", "HEAD", &Batch::default()).unwrap(), crate_level, "a graph that answers {name} falls back to the crate-level selection");
    }
}

const TREE_WIDE_UNITS: [&str; 3] = [
    "code_health::habits_tests::the_north_tree_keeps_its_rust_habits",
    "runtime::reuse::tests::every_file_a_source_embeds_at_compile_time_counts_as_a_rust_input",
    "runtime::reuse::tests::no_source_or_test_embeds_a_table_the_runtime_reads_from_its_tree",
];

// KEEP(bug): on 2026-11-08 n933-denylist, q902-conflicts and n937-locks each landed with main red on the Rust habits test or the runtime::reuse tree scans, because their pre-queue selection followed a change cone or crate fallback that never reached tests judging every file (north#902)
#[test]
fn every_lane_selection_names_the_tree_wide_unit_tests_the_graph_and_the_crate_fallback_alike() {
    let directory = tempfile::tempdir().unwrap();
    let traced = graph_stub(directory.path(), "traced", r#"printf '%s' '{"tests":["north-land-checks land::slots::select::tests::queues"],"files":[]}'"#);
    let untraced = graph_stub(directory.path(), "untraced", "exit 2");
    let table = read(tree_wide_units::TABLE);
    for program in [&traced, &untraced] {
        let selection = selection_asking(program, root(), "HEAD", "HEAD", &Batch::default()).unwrap();
        for test in TREE_WIDE_UNITS {
            assert!(selection.summary().contains(test), "{program:?}: the selection line names {test}: {}", selection.summary());
            assert!(table.contains(test), "{test} is declared in {}", tree_wide_units::TABLE);
        }
        assert!(selection.packages().is_superset(&["north-code-health".to_owned(), "north-runtime".to_owned()].into()), "{program:?}: their packages are built");
    }
    let graph = selection_asking(&traced, root(), "HEAD", "HEAD", &Batch::default()).unwrap().arguments();
    for test in TREE_WIDE_UNITS {
        assert!(graph.contains(&format!("test(={test})")), "the graph filter runs {test}: {graph}");
    }
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let packages = plan.names().chain([ROOT]).collect();
    for bad in ["north-runtime runtime::reuse::tests::x\n", "no-such-crate a::b # tree-wide: why\n", "north-runtime a::b # tree-wide:\n", "north-runtime a::b # tree-wide: why\nnorth-runtime a::b # tree-wide: again\n"] {
        assert!(tree_wide_units::Units::parse(bad, &packages).is_err(), "{bad:?} is refused");
    }
}

// KEEP(bug): a declared tree-wide unit test that was renamed or moved selects nothing, and the lane then skips the scan it was declared for (north#902)
#[test]
fn every_declared_tree_wide_unit_test_still_exists_in_its_package() {
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let units = tree_wide_units::Units::parse(&read(tree_wide_units::TABLE), &plan.names().chain([ROOT]).collect()).unwrap();
    for entry in units.names().split(", ") {
        let (package, test) = entry.split_once(' ').unwrap();
        let name = test.rsplit("::").next().unwrap();
        let mut pending = vec![root().join("crates").join(package).join("src")];
        let mut found = false;
        while let Some(path) = pending.pop() {
            if path.is_dir() {
                pending.extend(std::fs::read_dir(&path).unwrap().flatten().map(|entry| entry.path()));
            } else {
                found |= path.extension().is_some_and(|extension| extension == "rs") && std::fs::read_to_string(&path).unwrap().contains(&format!("fn {name}()"));
            }
        }
        assert!(found, "{package} declares no test fn {name}");
    }
}
