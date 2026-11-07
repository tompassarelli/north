use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use north_landing::land::slots::it_modules::{Plan, STRUCTURE, TABLE, Table};

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
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
    let table = Table::parse(&read(TABLE), &plan).unwrap();
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
