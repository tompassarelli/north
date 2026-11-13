use std::collections::{BTreeMap, BTreeSet};

use crate::env;
use crate::host_class;
use crate::rule::{rule, Rule, RuleKind};
use crate::sources::{self, Export, Owners};
use crate::workspace::{DepKind, Kind, Package, Target, Workspace};

pub struct Buckfile {
    pub dir: String,
    pub tree: Vec<String>,
    pub features: Vec<String>,
    pub rustc_flags: Vec<String>,
    pub rules: Vec<Rule>,
    pub exports: BTreeSet<Export>,
}

pub fn plan(workspace: &Workspace, lint_flags: &BTreeMap<String, Vec<String>>) -> Result<Vec<Buckfile>, String> {
    let owners = Owners::new(workspace.packages.iter().map(|package| package.dir.clone()));
    let mut exports: BTreeMap<String, BTreeSet<Export>> = BTreeMap::new();
    let mut files = Vec::new();
    for package in &workspace.packages {
        let rules = package_rules(workspace, package, &owners, &mut exports)?;
        let mut rustc_flags = lint_flags.get(&package.dir).cloned().unwrap_or_default();
        rustc_flags.extend(check_cfg(package));
        files.push(Buckfile { dir: package.dir.clone(), tree: Vec::new(), features: package.features.clone(), rustc_flags, rules, exports: BTreeSet::new() });
    }
    let tree: Vec<String> = files.iter().map(|file| if file.dir.is_empty() { ":files".to_owned() } else { format!("//{}:files", file.dir) }).collect();
    for file in files.iter_mut().filter(|file| file.dir.is_empty()) {
        file.tree.clone_from(&tree);
    }
    for file in &mut files {
        file.exports = exports.remove(&file.dir).unwrap_or_default();
    }
    Ok(files)
}

fn check_cfg(package: &Package) -> [String; 2] {
    let values: Vec<String> = package.declared_features.iter().map(|feature| format!("\"{feature}\"")).collect();
    ["--check-cfg=cfg(docsrs,test)".to_owned(), format!("--check-cfg=cfg(feature, values({}))", values.join(", "))]
}

struct Names<'a> {
    package: &'a Package,
    lib: Option<&'a Target>,
}

impl Names<'_> {
    fn binary(&self, target: &Target) -> String {
        if self.lib.is_some() && target.name == self.package.name { format!("{}-bin", target.name) } else { target.name.clone() }
    }

    fn rule(&self, target: &Target) -> String {
        match target.kind {
            Kind::Lib | Kind::ProcMacro => self.package.name.clone(),
            Kind::Bin => self.binary(target),
            Kind::Test => format!("{}-test-{}", self.package.name, target.name),
            Kind::Example => format!("{}-example-{}", self.package.name, target.name),
        }
    }
}

fn package_rules(workspace: &Workspace, package: &Package, owners: &Owners, exports: &mut BTreeMap<String, BTreeSet<Export>>) -> Result<Vec<Rule>, String> {
    let lib = package.targets.iter().find(|target| is_library(target.kind));
    let names = Names { package, lib };
    let binaries: BTreeMap<String, String> = package.targets.iter().filter(|target| target.kind == Kind::Bin).map(|target| (target.name.clone(), names.binary(target))).collect();
    let mut targets: Vec<&Target> = package.targets.iter().collect();
    targets.sort_by(|left, right| left.kind.cmp(&right.kind).then_with(|| left.name.cmp(&right.name)));
    let mut rules = Vec::new();
    for target in targets {
        let tree = sources::tree(&workspace.root, package, target, owners, exports)?;
        let name = names.rule(target);
        let own_lib = lib.filter(|_| !is_library(target.kind)).map(|_| package);
        let context = env::Context { package, target, binaries: &binaries, unittest: false };
        let integration = (target.kind == Kind::Test).then(|| format!("{}::{}", package.name, target.name));
        let mut built = rule(rule_kind(target.kind), name.clone(), integration, &tree, &context, own_lib, dep_kinds(target.kind))?;
        built.skip = host_class::skipped(package, target, false);
        rules.push(built);
        if target.test && matches!(target.kind, Kind::Lib | Kind::ProcMacro | Kind::Bin) {
            let context = env::Context { unittest: true, ..context };
            let binary_id = if target.kind == Kind::Bin { format!("{}::bin/{}", package.name, target.name) } else { package.name.clone() };
            let mut unittest = rule(RuleKind::Test, format!("{name}-unittest"), Some(binary_id), &tree, &context, own_lib, &[DepKind::Normal, DepKind::Dev])?;
            unittest.skip = host_class::skipped(package, target, true);
            rules.push(unittest);
        }
    }
    Ok(rules)
}

fn is_library(kind: Kind) -> bool {
    matches!(kind, Kind::Lib | Kind::ProcMacro)
}

fn rule_kind(kind: Kind) -> RuleKind {
    match kind {
        Kind::Lib => RuleKind::Library,
        Kind::ProcMacro => RuleKind::ProcMacro,
        Kind::Bin | Kind::Example => RuleKind::Binary,
        Kind::Test => RuleKind::Test,
    }
}

fn dep_kinds(kind: Kind) -> &'static [DepKind] {
    match kind {
        Kind::Lib | Kind::ProcMacro | Kind::Bin => &[DepKind::Normal],
        Kind::Test | Kind::Example => &[DepKind::Normal, DepKind::Dev],
    }
}
