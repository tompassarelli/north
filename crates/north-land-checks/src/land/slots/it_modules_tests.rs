use super::*;

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
    assert_eq!(selection, Selection::Modules(expected), "the probe runs the modules crates.txt maps to its crates");
    assert!(selection.arguments().starts_with("--test it -E kind(lib)|kind(bin)|(binary_id(north::it)&test(/"), "the root package's unit tests still run");
    let modules = |names: &[&str]| Selection::Modules(names.iter().map(|name| (*name).to_owned()).chain(["affected_modules", "agent_text_reads"].map(str::to_owned)).collect());
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
    let selection = selection_asking(&traced, root(), "HEAD", "HEAD").unwrap();
    assert_eq!(
        selection.arguments(),
        "--test it -E (binary_id(north-base::bin/probe)&(test(=tests::reads_home)))|(binary_id(north-land-checks)&(test(=land::slots::select::tests::queues)))|(binary_id(north::it)&(test(=fleet::launch::refuses_a_full_window)))|(package(north-land-checks)&test(/(^|::)narrows_to_ids$/))|(binary_id(north::it)&test(/^(affected_modules|agent_text_reads)::/))",
        "a traced lane runs its traced and always tests in the binaries the land check builds, the tests it adds, and the tree-wide tests/it modules"
    );
    assert!(selection.runs_module("agent_text_reads") && selection.runs_module("fleet") && !selection.runs_module("safe_push"), "a traced lane keeps the tree-wide modules that coverage cannot link to a change (north#706)");
    assert_eq!(selection.packages(), ["north", "north-base", "north-land-checks"].map(str::to_owned).into(), "the packages of the selected tests are built");
    let crate_level = select(&read(STRUCTURE), &read(TABLE), &spawners(root()), &[]).unwrap();
    for (name, body) in [("untraced", "echo '{\"tests\":[]}'; exit 3"), ("missing", "exit 2")] {
        let program = graph_stub(directory.path(), name, body);
        assert_eq!(selection_asking(&program, root(), "HEAD", "HEAD").unwrap(), crate_level, "a graph that answers {name} falls back to the crate-level selection");
    }
}
