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
