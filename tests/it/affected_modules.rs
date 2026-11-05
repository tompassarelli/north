use std::collections::BTreeSet;
use std::path::Path;

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

