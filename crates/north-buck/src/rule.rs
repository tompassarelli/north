use std::collections::{BTreeMap, BTreeSet};

use crate::env;
use crate::sources::Tree;
use crate::workspace::{DepKind, Package};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleKind {
    Library,
    ProcMacro,
    Binary,
    Test,
}

/// Which libtest cases a test rule runs: `skip` leaves named cases to another rule, `only` runs just the named cases, and `host` runs them on the invoking host inside the dependency box.
#[derive(Default)]
pub struct Cases {
    pub only: Vec<String>,
    pub skip: Vec<String>,
    pub host: bool,
}

pub struct Rule {
    pub kind: RuleKind,
    pub name: String,
    pub crate_name: String,
    pub crate_root: String,
    pub tree: Tree,
    pub edition: String,
    pub env: BTreeMap<String, String>,
    pub run_env: BTreeMap<String, String>,
    pub resources: Vec<String>,
    pub binary_id: Option<String>,
    pub deps: Vec<String>,
    pub named_deps: BTreeMap<String, String>,
    pub cases: Cases,
}

pub(crate) fn rule(kind: RuleKind, name: String, binary_id: Option<String>, tree: &Tree, context: &env::Context, own_lib: Option<&Package>, kinds: &[DepKind]) -> Result<Rule, String> {
    let target = context.target;
    let crate_name = target.name.replace('-', "_");
    let env = env::values(context, &tree.reads, &crate_name)?;
    let run_env = env::run_values(context);
    let mut deps = BTreeSet::new();
    let mut named_deps = BTreeMap::new();
    if let Some(package) = own_lib {
        deps.insert(format!(":{}", package.name));
    }
    for dep in context.package.deps.iter().filter(|dep| dep.kinds.iter().any(|kind| kinds.contains(kind))) {
        if dep.package == context.package.name {
            continue;
        }
        let label = match &dep.first_party_dir {
            Some(dir) => format!("//{dir}:{}", dep.package),
            None => format!("//third-party:{}", dep.package),
        };
        if dep.extern_name == dep.lib_crate {
            deps.insert(label);
        } else {
            named_deps.insert(dep.extern_name.clone(), label);
        }
    }
    let crate_root = tree.path_of(&target.src_path);
    let resources = if kind == RuleKind::Test { context.package.test_resources.clone() } else { Vec::new() };
    Ok(Rule { kind, name, crate_name, crate_root, tree: tree.clone(), edition: target.edition.clone(), env, run_env, resources, binary_id, deps: deps.into_iter().collect(), named_deps, cases: Cases::default() })
}
