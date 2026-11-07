use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use north_base::build_inputs::rust_build_input;
use regex::Regex;
use toml_edit::{DocumentMut, Item};

use super::select::changed;

pub const IT_ENV: &str = "SAFE_PUSH_LAND_IT";
pub const TABLE: &str = "tests/it/crates.txt";
pub const STRUCTURE: &str = "structure.toml";
pub const ROOT: &str = "north";
const UNIT: &str = "kind(lib)|kind(bin)";
const IT: &str = "binary_id(north::it)";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Every,
    Modules(BTreeSet<String>),
}

impl Selection {
    pub fn arguments(&self) -> String {
        match self {
            Self::Every => format!("--test it -E {UNIT}|{IT}"),
            Self::Modules(modules) if modules.is_empty() => format!("-E {UNIT}"),
            Self::Modules(modules) => format!("--test it -E {UNIT}|({IT}&test(/{}/))", modules.iter().map(|module| format!("{module}::")).collect::<Vec<_>>().join("|")),
        }
    }
}

struct Planned {
    name: String,
    globs: Vec<Regex>,
}

pub struct Plan {
    crates: Vec<Planned>,
    dependents: BTreeMap<String, BTreeSet<String>>,
}

impl Plan {
    pub fn parse(structure: &str) -> Result<Self, String> {
        let document: DocumentMut = structure.parse().map_err(|error: toml_edit::TomlError| format!("{STRUCTURE}: {error}"))?;
        let tables = document.get("crate_dag").and_then(|dag| dag.get("crate")).and_then(Item::as_array_of_tables).ok_or_else(|| format!("{STRUCTURE}: no [[crate_dag.crate]] tables"))?;
        let mut crates = Vec::new();
        let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for table in tables {
            let name = table.get("name").and_then(Item::as_str).ok_or_else(|| format!("{STRUCTURE}: a crate_dag.crate has no name"))?.to_owned();
            let globs = strings(table.get("paths"), &name, "paths")?.iter().map(|pattern| glob(pattern)).collect::<Result<_, _>>()?;
            for dependency in strings(table.get("may_use"), &name, "may_use")? {
                dependents.entry(dependency).or_default().insert(name.clone());
            }
            crates.push(Planned { name, globs });
        }
        Ok(Self { crates, dependents })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.crates.iter().map(|planned| planned.name.as_str())
    }

    fn owner(&self, file: &str) -> Option<&str> {
        if let Some(planned) = self.crates.iter().find(|planned| planned.globs.iter().any(|glob| glob.is_match(file))) {
            return Some(&planned.name);
        }
        let directory = file.strip_prefix("crates/")?.split('/').next()?;
        self.names().find(|name| *name == directory)
    }

    pub fn closure(&self, changed: BTreeSet<String>) -> BTreeSet<String> {
        let mut affected = changed.clone();
        let mut pending: Vec<String> = changed.into_iter().collect();
        while let Some(name) = pending.pop() {
            for dependent in self.dependents.get(&name).into_iter().flatten() {
                if affected.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
        affected
    }
}

pub struct Table {
    modules: BTreeMap<String, BTreeSet<String>>,
}

const SPAWNS_NORTH: &str = "CARGO_BIN_EXE_north";
const NARROW: &str = "narrow:";
const TREE_WIDE: &str = "tree-wide:";
const TESTS: &str = "tests/it";

impl Table {
    pub fn parse(table: &str, plan: &Plan, spawners: &BTreeSet<String>) -> Result<Self, String> {
        let known: BTreeSet<&str> = plan.names().chain([ROOT]).collect();
        let mut modules = BTreeMap::new();
        for (index, line) in table.lines().enumerate() {
            let (line, comment) = line.split_once(" #").unwrap_or((line, ""));
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            let module = words.next().unwrap_or_default().to_owned();
            let crates: BTreeSet<String> = words.map(str::to_owned).collect();
            if crates.is_empty() {
                return Err(format!("{TABLE}:{}: {module} names no crate", index + 1));
            }
            if let Some(unknown) = crates.iter().find(|name| !known.contains(name.as_str())) {
                return Err(format!("{TABLE}:{}: {module} names {unknown}, which is neither {ROOT} nor a [[crate_dag.crate]] in {STRUCTURE}", index + 1));
            }
            let narrow = comment.trim().strip_prefix(NARROW).map(str::trim);
            if narrow.is_some_and(str::is_empty) {
                return Err(format!("{TABLE}:{}: {module} has a narrow marker with no reason", index + 1));
            }
            let tree_wide = comment.trim().strip_prefix(TREE_WIDE).map(str::trim);
            if tree_wide.is_some_and(str::is_empty) {
                return Err(format!("{TABLE}:{}: {module} has a tree-wide marker with no reason", index + 1));
            }
            if narrow.is_some() && !spawners.contains(&module) {
                return Err(format!("{TABLE}:{}: {module} does not spawn the north binary, so a narrow reason on it means nothing", index + 1));
            }
            let crates = if tree_wide.is_some() || (spawners.contains(&module) && narrow.is_none()) { known.iter().map(|name| (*name).to_owned()).collect() } else { crates };
            if modules.insert(module.clone(), crates).is_some() {
                return Err(format!("{TABLE}:{}: {module} is listed twice", index + 1));
            }
        }
        Ok(Self { modules })
    }

    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    pub fn crates(&self, module: &str) -> Option<&BTreeSet<String>> {
        self.modules.get(module)
    }
}

pub fn select(structure: &str, table: &str, spawners: &BTreeSet<String>, files: &[String]) -> Result<Selection, String> {
    let plan = Plan::parse(structure)?;
    let table = Table::parse(table, &plan, spawners)?;
    let mut changed = BTreeSet::new();
    for file in files {
        match plan.owner(file) {
            Some(name) => {
                changed.insert(name.to_owned());
            }
            None if file.starts_with("docs/") && !rust_build_input(file) => {
                changed.insert(ROOT.to_owned());
            }
            None => return Ok(Selection::Every),
        }
    }
    let affected = plan.closure(changed);
    if plan.names().all(|name| affected.contains(name)) {
        return Ok(Selection::Every);
    }
    let modules: BTreeSet<String> = table.modules.iter().filter(|(_, crates)| !crates.is_disjoint(&affected)).map(|(module, _)| module.clone()).collect();
    if modules.len() == table.modules.len() {
        return Ok(Selection::Every);
    }
    Ok(Selection::Modules(modules))
}

pub fn selection(root: &Path, base: &str, head: &str) -> Result<Selection, String> {
    let read = |path: &str| std::fs::read_to_string(root.join(path)).map_err(|error| format!("{path}: {error}"));
    select(&read(STRUCTURE)?, &read(TABLE)?, &spawners(root), &changed(root, base, head, &[]))
}

pub fn spawners(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.join(TESTS)];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
        } else if path.extension().is_some_and(|extension| extension == "rs") && std::fs::read_to_string(&path).is_ok_and(|text| text.contains(SPAWNS_NORTH)) {
            let relative = path.strip_prefix(root.join(TESTS)).unwrap_or(&path);
            if let Some(module) = relative.components().next().and_then(|first| first.as_os_str().to_str()) {
                found.insert(module.trim_end_matches(".rs").to_owned());
            }
        }
    }
    found
}

fn strings(item: Option<&Item>, name: &str, key: &str) -> Result<Vec<String>, String> {
    let array = item.and_then(Item::as_array).ok_or_else(|| format!("{STRUCTURE}: crate_dag.crate {name} {key} must be a list of strings"))?;
    array.iter().map(|value| value.as_str().map(str::to_owned).ok_or_else(|| format!("{STRUCTURE}: crate_dag.crate {name} {key} must be a list of strings"))).collect()
}

fn glob(pattern: &str) -> Result<Regex, String> {
    let mut out = String::from("^");
    let mut rest = pattern;
    while let Some(character) = rest.chars().next() {
        let (piece, width) = match character {
            '*' if rest.starts_with("**/") => ("(?:.*/)?".to_owned(), 3),
            '*' if rest.starts_with("**") => (".*".to_owned(), 2),
            '*' => ("[^/]*".to_owned(), 1),
            '?' => ("[^/]".to_owned(), 1),
            other => (regex::escape(other.encode_utf8(&mut [0; 4])), other.len_utf8()),
        };
        out.push_str(&piece);
        rest = &rest[width..];
    }
    out.push('$');
    Regex::new(&out).map_err(|error| format!("{STRUCTURE}: glob {pattern}: {error}"))
}
