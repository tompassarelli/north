use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::base::Base;
use super::plan::{Declaration, Edit, Plan, Symbol};

const LOCK: &str = "Cargo.lock";
const ROOTS: [&str; 2] = ["lib", "main"];
const TARGET_DIRECTORIES: [&str; 4] = ["bin", "tests", "benches", "examples"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shared {
    pub files: BTreeSet<String>,
    pub modules: BTreeSet<String>,
    pub dependencies: BTreeSet<(String, String)>,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Part {
    pub names: Vec<String>,
    pub summaries: Vec<String>,
    pub files: BTreeSet<String>,
    pub crates: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Split {
    pub shared: Shared,
    pub parts: Vec<Part>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Refusal(pub Vec<String>);

impl Refusal {
    pub fn line(&self) -> String {
        self.0.join("; ")
    }
}

impl Split {
    pub fn new(shared: Shared, parts: Vec<Part>, base: &dyn Base) -> Result<Self, Refusal> {
        let mut holders: BTreeMap<&str, &str> = BTreeMap::new();
        let mut refused = Vec::new();
        for part in &parts {
            let name = part.names.first().map_or("", String::as_str);
            for file in part.files.iter().filter(|file| !base.merged_as_set(file)) {
                if let Some(holder) = holders.insert(file, name) {
                    refused.push(format!("parts {holder} and {name} both name {file}"));
                }
            }
        }
        if refused.is_empty() { Ok(Self { shared, parts }) } else { Err(Refusal(refused)) }
    }
}

fn manifest(path: &str) -> bool {
    path == LOCK || path.rsplit('/').next() == Some("Cargo.toml")
}

fn module_parent(path: &str, exists: &dyn Fn(&str) -> bool) -> Option<Option<String>> {
    let file = Path::new(path);
    if file.extension().is_none_or(|extension| extension != "rs") || file.file_name().is_some_and(|name| name == "build.rs") {
        return Some(None);
    }
    let stem = file.file_stem()?.to_str()?;
    let directory = file.parent()?.to_str()?;
    let last = directory.rsplit('/').next().unwrap_or(directory);
    if ROOTS.contains(&stem) || TARGET_DIRECTORIES.contains(&last) {
        return Some(None);
    }
    let (directory, stem) = if stem == "mod" { directory.rsplit_once('/')? } else { (directory, stem) };
    let mut candidates: Vec<String> = stem.strip_suffix("_tests").map(|tested| format!("{directory}/{tested}.rs")).into_iter().collect();
    candidates.extend(["lib", "main", "mod"].map(|root| format!("{directory}/{root}.rs")));
    candidates.push(format!("{directory}.rs"));
    candidates.into_iter().find(|candidate| exists(candidate)).map(Some)
}

struct Planned<'p> {
    plan: &'p Plan,
    base: &'p dyn Base,
    created: BTreeSet<&'p str>,
    new_crates: Vec<String>,
}

impl<'p> Planned<'p> {
    fn new(plan: &'p Plan, base: &'p dyn Base) -> Self {
        let created: BTreeSet<&str> = plan.parts.iter().flat_map(|part| &part.files).filter(|edit| edit.new).map(|edit| edit.path.as_str()).collect();
        let new_crates = created.iter().filter_map(|path| path.strip_suffix("/Cargo.toml")).map(|root| format!("{root}/")).collect();
        Self { plan, base, created, new_crates }
    }

    fn exists(&self, path: &str) -> bool {
        self.base.tracked(path) || self.created.contains(path)
    }

    fn crate_of(&self, path: &str) -> Option<&str> {
        if self.new_crates.iter().any(|root| path.starts_with(root.as_str())) { None } else { self.base.crate_of(path) }
    }

    fn edits(&self) -> impl Iterator<Item = (&'p str, &'p Edit)> {
        self.plan.parts.iter().flat_map(|part| part.files.iter().map(move |edit| (part.name.as_str(), edit)))
    }

    fn checked_edit(&self, part: &str, edit: &Edit, refused: &mut Vec<String>) {
        match (edit.new, self.base.tracked(&edit.path)) {
                (true, true) => refused.push(format!("part {part} creates {} but the base already has it", edit.path)),
                (false, false) => refused.push(format!("part {part} changes {} but the base has no such file (mark it new if the task creates it)", edit.path)),
                _ => {}
            }
            for symbol in edit.changes.iter().filter(|symbol| self.base.defining(symbol).is_empty()) {
                refused.push(format!("part {part} changes {} in {} but the base defines no {}", symbol.shown(), edit.path, symbol.shown()));
            }
            if edit.new && module_parent(&edit.path, &|candidate| self.exists(candidate)).is_none() {
                refused.push(format!("part {part} creates {} but neither the base nor the plan holds the module file that declares it", edit.path));
            }
    }

    fn checked(&self) -> Vec<String> {
        let mut refused = Vec::new();
        for (part, edit) in self.edits() {
            self.checked_edit(part, edit, &mut refused);
        }
        let edited: BTreeSet<&str> = self.edits().map(|(_, edit)| edit.path.as_str()).collect();
        for declaration in &self.plan.shared {
            if !edited.contains(declaration.path.as_str()) {
                refused.push(format!("the shared {} is declared in {}, which no part fills, so its stub would stay on main", declaration.symbol.shown(), declaration.path));
            }
        }
        refused
    }

    fn owners(&self, symbol: &Symbol) -> BTreeSet<String> {
        let added: BTreeSet<String> = self.edits().filter(|(_, edit)| edit.adds.iter().any(|added| added.name == symbol.name)).map(|(_, edit)| edit.path.clone()).collect();
        let declared: BTreeSet<String> = self.plan.shared.iter().filter(|declaration| declaration.symbol.name == symbol.name).map(|declaration| declaration.path.clone()).collect();
        if !added.is_empty() || !declared.is_empty() { added.union(&declared).cloned().collect() } else { self.base.defining(symbol) }
    }

    fn depend(&self, part: &str, edit: &Edit, shared: &mut Shared, refused: &mut Vec<String>) {
        let Some(user) = self.crate_of(&edit.path) else { return };
        for symbol in &edit.uses {
            for owner in self.owners(symbol).iter().filter_map(|file| self.crate_of(file)).filter(|owner| *owner != user && !self.base.sees(user, owner)) {
                if self.base.sees(owner, user) {
                    refused.push(format!("part {part} uses {} from {owner} in {}, but {owner} depends on {user}, so the dependency would be a cycle", symbol.shown(), edit.path));
                } else {
                    shared.dependencies.insert((user.to_owned(), owner.to_owned()));
                    shared.files.extend(self.base.manifest_of(user));
                    shared.files.insert(LOCK.to_owned());
                }
            }
        }
    }

    fn shared(&self) -> Result<Shared, Refusal> {
        let mut shared = Shared { declarations: self.plan.shared.clone(), ..Shared::default() };
        shared.files.extend(self.plan.shared.iter().map(|declaration| declaration.path.clone()));
        let mut refused = Vec::new();
        for (part, edit) in self.edits() {
            if manifest(&edit.path) {
                shared.files.insert(edit.path.clone());
            }
            if edit.new {
                shared.modules.insert(edit.path.clone());
                shared.files.insert(edit.path.clone());
                if let Some(Some(parent)) = module_parent(&edit.path, &|candidate| self.exists(candidate)) {
                    shared.files.insert(parent);
                }
            }
            self.depend(part, edit, &mut shared, &mut refused);
        }
        if refused.is_empty() { Ok(shared) } else { Err(Refusal(refused)) }
    }

    fn closed(&self, edits: &[Edit]) -> BTreeSet<String> {
        let mut files: BTreeSet<String> = edits.iter().map(|edit| edit.path.clone()).filter(|path| !manifest(path)).collect();
        for symbol in edits.iter().flat_map(|edit| &edit.changes) {
            let defining = self.base.defining(symbol);
            let owners: BTreeSet<&str> = defining.iter().filter_map(|file| self.base.crate_of(file)).collect();
            let referring = self.base.referring(symbol);
            let reached = referring.into_iter().filter(|file| self.base.crate_of(file).is_some_and(|user| owners.iter().any(|owner| self.base.sees(user, owner))));
            files.extend(reached.chain(defining.iter().cloned()));
        }
        files
    }

    fn parts(&self) -> Vec<Part> {
        let parts: Vec<Part> = self
            .plan
            .parts
            .iter()
            .map(|part| {
                let files = self.closed(&part.files);
                let crates = files.iter().filter_map(|file| self.crate_of(file)).map(str::to_owned).collect();
                Part { names: vec![part.name.clone()], summaries: vec![part.summary.clone()], files, crates }
            })
            .collect();
        let mut roots: Vec<usize> = (0..parts.len()).collect();
        let mut holders: BTreeMap<&str, usize> = BTreeMap::new();
        for (index, part) in parts.iter().enumerate() {
            for file in part.files.iter().filter(|file| !self.base.merged_as_set(file)) {
                if let Some(holder) = holders.insert(file, index) {
                    join(&mut roots, holder, index);
                }
            }
        }
        let declared: BTreeSet<&str> = self.plan.shared.iter().map(|declaration| declaration.symbol.name.as_str()).collect();
        for (adder, planned) in self.plan.parts.iter().enumerate() {
            let added: BTreeSet<&str> = planned.files.iter().flat_map(|edit| &edit.adds).map(|symbol| symbol.name.as_str()).filter(|name| !declared.contains(name)).collect();
            for (user, other) in self.plan.parts.iter().enumerate().filter(|(user, _)| *user != adder) {
                if other.files.iter().flat_map(|edit| &edit.uses).any(|symbol| added.contains(symbol.name.as_str())) {
                    join(&mut roots, adder, user);
                }
            }
        }
        let mut merged: BTreeMap<usize, Part> = BTreeMap::new();
        for (index, part) in parts.into_iter().enumerate() {
            let into = merged.entry(find(&mut roots, index)).or_default();
            into.names.extend(part.names);
            into.summaries.extend(part.summaries);
            into.files.extend(part.files);
            into.crates.extend(part.crates);
        }
        merged.into_values().filter(|part| !part.files.is_empty()).collect()
    }
}

fn find(roots: &mut [usize], mut index: usize) -> usize {
    while roots[index] != index {
        roots[index] = roots[roots[index]];
        index = roots[index];
    }
    index
}

fn join(roots: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(roots, a), find(roots, b));
    roots[a.max(b)] = a.min(b);
}

pub fn split(plan: &Plan, base: &dyn Base) -> Result<Split, Refusal> {
    let planned = Planned::new(plan, base);
    let refused = planned.checked();
    if !refused.is_empty() {
        return Err(Refusal(refused));
    }
    let shared = planned.shared()?;
    Split::new(shared, planned.parts(), base)
}
