use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use north_landing::land::slots::it_modules::{Plan, STRUCTURE, Selection, TABLE, Table, select, spawners};
use north_landing::land::slots::tree_wide_units::Units;

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

#[test]
fn every_tests_it_module_maps_to_crates_the_crate_dag_names() {
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let table = Table::parse(&read(TABLE), &plan, &spawners_of_tree()).unwrap();
    let declared: BTreeSet<String> = read("tests/it/main.rs").lines().filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';').map(str::to_owned)).collect();
    let mapped: BTreeSet<String> = table.modules().map(str::to_owned).collect();
    assert_eq!(declared.difference(&mapped).collect::<Vec<_>>(), Vec::<&String>::new(), "add each new tests/it module to {TABLE} with the crates it exercises");
    assert_eq!(mapped.difference(&declared).collect::<Vec<_>>(), Vec::<&String>::new(), "{TABLE} names a module tests/it/main.rs no longer declares");
}


fn sources(module: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let mut pending: Vec<PathBuf> = vec![root.join(format!("{module}.rs")), root.join(module)];
    let mut text = String::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
        } else if path.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            text.push_str(&std::fs::read_to_string(&path).unwrap());
        }
    }
    text
}

// KEEP(bug): the lander ran a lane that broke a test only north-capacity-host code reaches, because the table's safe_push line did not name the capacity crates the test imports, so the lane's candidate skipped the test and main went red (north#782)
#[test]
fn every_crate_a_tests_it_module_imports_is_one_the_table_selects_it_for() {
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let table = Table::parse(&read(TABLE), &plan, &spawners_of_tree()).unwrap();
    let mut unselected = Vec::new();
    for module in table.modules() {
        let text = sources(module).replace("north_core::foundation::shim", "");
        let imported: BTreeSet<String> = plan.names().filter(|name| text.contains(&format!("{}::", name.replace('-', "_")))).map(str::to_owned).collect();
        let selected = table.crates(module).unwrap();
        for name in imported.iter().filter(|name| !plan.closure(BTreeSet::from([(*name).clone()])).iter().any(|dependent| selected.contains(dependent))) {
            unselected.push(format!("{module} imports {name}"));
        }
    }
    assert_eq!(unselected, Vec::<String>::new(), "list each crate a module imports, or one that uses it, on the module's line of {TABLE}, so a change to that crate runs the module");
}

// KEEP(bug): 8c5d22002 changed north-capacity and north-capacity-host and the table's safe_push line did not name them, though the test spawns the north binary that links both, so the lane's candidate skipped it (north#782)
#[test]
fn a_module_that_spawns_the_north_binary_runs_for_any_crate_change_unless_its_line_says_why_not() {
    let files = ["crates/north-capacity/src/capacity/lease_lifecycle.rs".to_owned(), "crates/north-capacity-host/src/capacity/host/state.rs".to_owned()];
    let structure = read(STRUCTURE);
    let spawners = BTreeSet::from(["safe_push".to_owned()]);
    let modules = |names: &[&str]| Selection::Modules { modules: names.iter().map(|name| (*name).to_owned()).collect(), units: Units::default() };
    assert_eq!(select(&structure, "safe_push north-landing\nplain north-strategy-types\n", &spawners, &files).unwrap(), modules(&["safe_push"]), "a module that spawns the north binary links every crate");
    assert_eq!(select(&structure, "safe_push north-landing # narrow: reads only the tag path\nplain north-strategy-types\n", &spawners, &files).unwrap(), modules(&[]), "a narrow reason keeps the declared crates");
    assert!(select(&structure, "safe_push north-landing # narrow:\nplain north-strategy-types\n", &spawners, &files).is_err(), "a narrow marker needs a reason");
    assert!(select(&structure, "plain north-strategy-types # narrow: nothing to narrow\n", &spawners, &files).is_err(), "a module that does not spawn the binary has nothing to narrow");
    assert!(spawners_of_tree().contains("safe_push"), "the scan finds the modules that spawn the north binary");
}

fn spawners_of_tree() -> BTreeSet<String> {
    spawners(Path::new(env!("CARGO_MANIFEST_DIR")))
}

// KEEP(bug): f0e87b7c09 changed only north-dispatch, so the land check skipped agent_text_reads, which scans every source file, and the lane landed with main red until CI caught it (north#706)
#[test]
fn a_tree_wide_module_runs_for_a_lane_that_changes_any_crate() {
    let files = ["crates/north-dispatch/src/dispatch/stops.rs".to_owned(), "crates/north-dispatch/src/dispatch/tries.rs".to_owned()];
    let selection = select(&read(STRUCTURE), &read(TABLE), &spawners_of_tree(), &files).unwrap();
    let runs = |module: &str| selection.runs_module(module);
    assert!(runs("agent_text_reads") && runs("affected_modules"), "a replay of f0e87b7c09 selects the tree-wide modules: {selection:?}");
    assert!(select(&read(STRUCTURE), "plain north-strategy-types # tree-wide:\n", &BTreeSet::new(), &files).is_err(), "a tree-wide marker needs a reason");
}
