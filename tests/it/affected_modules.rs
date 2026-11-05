use std::collections::BTreeSet;
use std::path::Path;

use north_landing::land::slots::it_modules::{Plan, ROOT, STRUCTURE, Selection, TABLE, Table, select};

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

fn selection(files: &[&str]) -> Selection {
    select(&read(STRUCTURE), &read(TABLE), &files.iter().map(|file| (*file).to_owned()).collect::<Vec<_>>()).unwrap()
}

fn modules(files: &[&str]) -> BTreeSet<String> {
    match selection(files) {
        Selection::Modules(modules) => modules,
        Selection::Every => panic!("{files:?} selected every tests/it module"),
    }
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn every_tests_it_module_maps_to_crates_the_crate_dag_names() {
    let plan = Plan::parse(&read(STRUCTURE)).unwrap();
    let table = Table::parse(&read(TABLE), &plan).unwrap();
    let declared: BTreeSet<String> = read("tests/it/main.rs").lines().filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';').map(str::to_owned)).collect();
    let mapped: BTreeSet<String> = table.modules().map(str::to_owned).collect();
    assert_eq!(declared.difference(&mapped).collect::<Vec<_>>(), Vec::<&String>::new(), "add each new tests/it module to {TABLE} with the crates it exercises");
    assert_eq!(mapped.difference(&declared).collect::<Vec<_>>(), Vec::<&String>::new(), "{TABLE} names a module tests/it/main.rs no longer declares");
}

#[test]
fn a_north_runtime_change_alone_selects_only_the_modules_of_north_runtime_and_its_dependents() {
    let runtime = set(&["agents_sync_runtime_skew", "capacity_runtime_follow", "carry_matrix", "land_offload", "runtime_update"]);
    assert_eq!(modules(&["crates/north-runtime/src/runtime/bounded.rs"]), runtime);
    assert_eq!(modules(&["crates/north-runtime/src/land_offload/ship.rs", "crates/north-runtime/src/timers/tests.rs"]), runtime);
    assert_eq!(modules(&["crates/north-runtime/src/lib.rs", "crates/north-runtime/Cargo.toml"]), runtime, "the selection holds once north-runtime is its own crate");
    let table = Table::parse(&read(TABLE), &Plan::parse(&read(STRUCTURE)).unwrap()).unwrap();
    for module in table.modules().filter(|module| !runtime.contains(*module)) {
        assert!(!table.crates(module).unwrap().contains("north-runtime"), "{module} exercises north-runtime but was not selected");
    }
    assert_eq!(selection(&["crates/north-runtime/src/runtime/bounded.rs"]).arguments(), "--test it -- agents_sync_runtime_skew:: capacity_runtime_follow:: carry_matrix:: land_offload:: runtime_update::");
}

#[test]
fn a_hub_change_selects_the_modules_of_every_crate_that_uses_it() {
    let capacity = modules(&["crates/north-core/src/capacity/admission.rs"]);
    for module in ["capacity_aperture", "capacity_host", "laptop_holds", "agents_dispatch", "runtime_update", "agent_spawn_hook", "policy_replay_run"] {
        assert!(capacity.contains(module), "{module}: {capacity:?}");
    }
    for module in ["land_checks", "fleet_send", "threads_snapshot", "ledger_claude_runs", "proxy", "readme_links"] {
        assert!(!capacity.contains(module), "{module}: {capacity:?}");
    }
    assert_eq!(modules(&["crates/north-core/src/grounding/compose.rs"]), set(&["grounding_tier_b", "proxy", "recommendation_grounding_hook"]));
    assert_eq!(modules(&["crates/north-landing/src/land/slots/affected.rs"]).iter().filter(|module| !module.starts_with("land") && !module.starts_with("safe_push")).cloned().collect::<BTreeSet<_>>(), set(&["affected_modules", "follows_release_no_python", "autoland_rebased", "golden_path", "merge_driver_set", "provenance"]));
}

#[test]
fn a_root_base_build_input_or_harness_change_selects_every_module() {
    for file in ["src/main.rs", "tests/it/fleet_send.rs", "crates/north-base/src/build_inputs.rs", "Cargo.lock", "harness/defaults.toml", "bin/safe-push", "harness/skills/machine-capacity/scripts/capacity-run-queue-overload.recorded.json"] {
        assert_eq!(selection(&["crates/north-runtime/src/runtime/bounded.rs", file]), Selection::Every, "{file}");
    }
    assert_eq!(Selection::Every.arguments(), "--test it");
}

#[test]
fn a_docs_change_selects_the_modules_that_read_repo_files() {
    let docs = modules(&["docs/rationale/crate-split.md"]);
    for module in ["readme_links", "registry", "merge_driver_set"] {
        assert!(docs.contains(module), "{module}: {docs:?}");
    }
    assert!(!docs.contains("land_checks") && !docs.contains("runtime_update"), "{docs:?}");
}

#[test]
fn a_module_with_no_crate_or_an_unknown_crate_is_refused_and_no_selection_runs_nothing() {
    let structure = read(STRUCTURE);
    assert!(select(&structure, "one north-moon\n", &[]).unwrap_err().contains("north-moon, which is neither north"));
    assert!(select(&structure, "one\n", &[]).unwrap_err().contains("one names no crate"));
    assert!(select(&structure, "one north\none north-guard\n", &[]).unwrap_err().contains("one is listed twice"));
    let none = select(&structure, &format!("one {ROOT}\ntwo north-landing\n"), &["crates/north-guard/src/hook.rs".to_owned()]).unwrap();
    assert_eq!(none, Selection::Modules(BTreeSet::new()));
    assert_eq!(none.arguments(), "--test it -- --exact no-module-selected");
}
