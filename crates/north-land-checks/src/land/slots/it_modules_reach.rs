use std::collections::BTreeSet;

use north_base::build_inputs::rust_build_input;

use super::{DOCS, Plan, ROOT, SOURCES, Table};

pub(super) struct Reach {
    pub(super) modules: BTreeSet<String>,
    pub(super) every: bool,
}

enum Owner {
    Crate(String),
    Module(String),
    Outside,
}

fn owner(plan: &Plan, table: &Table, file: &str) -> Owner {
    if let Some(name) = plan.owner(file) {
        Owner::Crate(name.to_owned())
    } else if let Some(module) = table.module_of(file) {
        Owner::Module(module.to_owned())
    } else if file.starts_with(SOURCES) || (file.starts_with(DOCS) && !rust_build_input(file)) {
        Owner::Crate(ROOT.to_owned())
    } else {
        Owner::Outside
    }
}

pub(super) fn reach(plan: &Plan, table: &Table, files: &[String]) -> Reach {
    let mut changed = BTreeSet::new();
    let mut edited = BTreeSet::new();
    let mut outside = false;
    for file in files {
        match owner(plan, table, file) {
            Owner::Crate(name) => _ = changed.insert(name),
            Owner::Module(module) => _ = edited.insert(module),
            Owner::Outside => outside = true,
        }
    }
    let affected = plan.closure(changed);
    if !edited.is_empty() {
        edited.extend(table.tree_wide.iter().cloned());
    }
    let modules: BTreeSet<String> = table.modules.iter().filter(|(_, crates)| !crates.is_disjoint(&affected)).map(|(module, _)| module.clone()).chain(edited).collect();
    let every = outside || plan.names().all(|name| affected.contains(name)) || modules.len() == table.modules.len();
    Reach { modules, every }
}

pub(super) fn compiled(file: &str) -> bool {
    file.ends_with(".rs") || file.ends_with("/Cargo.toml") || file == "Cargo.toml"
}
