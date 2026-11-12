use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use regex::Regex;
use toml_edit::{DocumentMut, Item};

use super::graph_selection::{self, Batch, Tests};
use super::guard_suites::{self, Floor, Suites};
use super::select::changed;
use super::shared_build::live_runtime;
use super::tree_wide_units::{self, Units};

#[path = "it_modules_reach.rs"]
mod reach;
use reach::{Reach, compiled, reach};

pub const IT_ENV: &str = "SAFE_PUSH_LAND_IT";
pub const TABLE: &str = "tests/it/crates.txt";
pub const STRUCTURE: &str = "structure.toml";
pub const ROOT: &str = "north";
const UNIT: &str = "kind(lib)|kind(bin)";
const IT: &str = "binary_id(north::it)";
const EVERY_IT: &str = "kind(test)";
const SELECTION_INPUTS: [&str; 5] = [TABLE, STRUCTURE, tree_wide_units::TABLE, guard_suites::TABLE, ".safe-push"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Every,
    Modules { modules: BTreeSet<String>, units: Units, floor: Floor },
    Tests(Tests),
    NoTest(String),
}

#[derive(Clone, Copy, Debug)]
pub enum Stage<'a> {
    BeforePush,
    AfterLanding(&'a Batch),
}

static NO_BATCH: Batch = Batch { lanes: Vec::new(), ran: Vec::new() };

impl Stage<'_> {
    fn batch(&self) -> &Batch {
        match self {
            Self::BeforePush => &NO_BATCH,
            Self::AfterLanding(batch) => batch,
        }
    }
}

impl Selection {
    pub fn arguments(&self) -> String {
        match self {
            Self::Every => format!("--tests -E {UNIT}|{EVERY_IT}"),
            Self::Modules { modules, floor, .. } => {
                let traced = (!modules.is_empty()).then(|| format!("({IT}&test(/{}/))", modules.iter().map(|module| format!("{module}::")).collect::<Vec<_>>().join("|")));
                let extra = (!floor.is_empty()).then(|| floor.filter());
                let filter = std::iter::once(UNIT.to_owned()).chain(traced).chain(extra).collect::<Vec<_>>().join("|");
                format!("{}-E {filter}", if !modules.is_empty() || floor.has_it_case() { "--tests " } else { "" })
            }
            Self::Tests(tests) => tests.arguments(),
            Self::NoTest(_) => "--tests -E none()".to_owned(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Tests(tests) => tests.summary(),
            Self::Modules { units, floor, .. } if !units.is_empty() || !floor.is_empty() => format!("{}, and the whole packages of the tree-wide unit tests {} and of the guard suites {}", self.arguments(), units.names(), floor.names()),
            Self::NoTest(reason) => format!("no test, since {reason}"),
            Self::Every | Self::Modules { .. } => self.arguments(),
        }
    }

    pub fn with_units(self, units: Units) -> Self {
        match self {
            Self::Every => Self::Every,
            Self::Modules { modules, floor, .. } => Self::Modules { modules, units, floor },
            Self::Tests(tests) => Self::Tests(tests.with_units(units)),
            Self::NoTest(reason) => Self::NoTest(reason),
        }
    }

    pub fn with_floor(self, floor: Floor) -> Self {
        match self {
            Self::Every => Self::Every,
            Self::Modules { modules, units, .. } => Self::Modules { modules: modules.into_iter().chain(floor.it.iter().cloned()).collect(), units, floor },
            Self::Tests(tests) => Self::Tests(tests.with_floor(floor)),
            Self::NoTest(reason) => Self::NoTest(reason),
        }
    }

    pub fn runs_module(&self, module: &str) -> bool {
        match self {
            Self::Every => true,
            Self::Modules { modules, .. } => modules.contains(module),
            Self::Tests(tests) => tests.runs_module(module),
            Self::NoTest(_) => false,
        }
    }

    pub fn packages(&self) -> BTreeSet<String> {
        match self {
            Self::Tests(tests) => tests.packages(),
            Self::Modules { units, floor, .. } => units.packages().into_iter().chain(floor.packages()).collect(),
            Self::Every | Self::NoTest(_) => BTreeSet::new(),
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
    tree_wide: BTreeSet<String>,
}

const SPAWNS_NORTH: &str = "CARGO_BIN_EXE_north";
const NARROW: &str = "narrow:";
pub(super) const TREE_WIDE: &str = "tree-wide:";
const TESTS: &str = "tests/it";
const SOURCES: &str = "src/";
const DOCS: &str = "docs/";
const SUPPORT: &str = "support";

impl Table {
    pub fn parse(table: &str, plan: &Plan, spawners: &BTreeSet<String>) -> Result<Self, String> {
        let known: BTreeSet<&str> = plan.names().chain([ROOT]).collect();
        let mut modules = BTreeMap::new();
        let mut tree_wide_modules = BTreeSet::new();
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
            if tree_wide.is_some() {
                tree_wide_modules.insert(module.clone());
            }
            let crates = if tree_wide.is_some() || (spawners.contains(&module) && narrow.is_none()) { known.iter().map(|name| (*name).to_owned()).collect() } else { crates };
            if modules.insert(module.clone(), crates).is_some() {
                return Err(format!("{TABLE}:{}: {module} is listed twice", index + 1));
            }
        }
        Ok(Self { modules, tree_wide: tree_wide_modules })
    }

    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    pub fn crates(&self, module: &str) -> Option<&BTreeSet<String>> {
        self.modules.get(module)
    }

    fn module_of(&self, file: &str) -> Option<&str> {
        let relative = file.strip_prefix(TESTS)?.strip_prefix('/')?;
        let first = match relative.split_once('/') {
            Some((directory, _)) => directory,
            None => relative.strip_suffix(".rs")?,
        };
        self.modules.get_key_value(first).map(|(module, _)| module.as_str()).filter(|module| *module != SUPPORT)
    }
}

pub fn select(structure: &str, table: &str, spawners: &BTreeSet<String>, files: &[String]) -> Result<Selection, String> {
    let plan = Plan::parse(structure)?;
    let table = Table::parse(table, &plan, spawners)?;
    let Reach { modules, every } = reach(&plan, &table, files);
    Ok(if every { Selection::Every } else { Selection::Modules { modules, units: Units::default(), floor: Floor::default() } })
}

pub fn selection(root: &Path, base: &str, head: &str, stage: Stage<'_>, away: Option<&graph_selection::Away<'_>>) -> Result<Selection, String> {
    selection_asking(&live_runtime(), root, base, head, stage, away)
}

pub fn selection_asking(program: &Path, root: &Path, base: &str, head: &str, stage: Stage<'_>, away: Option<&graph_selection::Away<'_>>) -> Result<Selection, String> {
    selection_asking_with(program, root, base, head, stage, |scope| graph_selection::list(root, away, scope))
}

/// [`selection_asking`] with its `cargo nextest list` step injected, so a test can judge the graph answer and the inclusion filter without building the workspace.
pub fn selection_asking_with(program: &Path, root: &Path, base: &str, head: &str, stage: Stage<'_>, listed: impl FnOnce(&graph_selection::Scope) -> Option<graph_selection::ListedTests>) -> Result<Selection, String> {
    let files = changed(root, base, head, &[])?;
    selection_of_files_with(program, root, base, head, &files, stage, listed)
}

pub fn selection_of_files_with(program: &Path, root: &Path, base: &str, head: &str, files: &[String], stage: Stage<'_>, listed: impl FnOnce(&graph_selection::Scope) -> Option<graph_selection::ListedTests>) -> Result<Selection, String> {
    let read = |path: &str| std::fs::read_to_string(root.join(path)).map_err(|error| format!("{path}: {error}"));
    let plan = Plan::parse(&read(STRUCTURE)?)?;
    let table = Table::parse(&read(TABLE)?, &plan, &spawners(root))?;
    let units = units(&read(tree_wide_units::TABLE)?, &plan)?;
    let floor = Suites::read(root)?.floor(files);
    let selection = match graph_selection::ask(program, root, base, head, stage.batch()) {
        Some(tests) if tests.is_empty() && matches!(stage, Stage::AfterLanding(_)) && floor.is_empty() && !files.iter().any(|file| compiled(file) || SELECTION_INPUTS.contains(&file.as_str())) => {
            return Ok(Selection::NoTest(format!("the change graph reaches no test from {}", files.join(", "))));
        }
        Some(tests) if !tests.is_empty() || matches!(stage, Stage::AfterLanding(_)) => {
            let compiled: Vec<String> = files.iter().filter(|file| compiled(file)).cloned().collect();
            let scope = tests.listing_scope();
            match tests.with_listed(listed(&scope)) {
                Some(tests) => Selection::Tests(tests.with_modules(table.tree_wide.iter().cloned().chain(reach(&plan, &table, &compiled).modules).collect())),
                None => select(&read(STRUCTURE)?, &read(TABLE)?, &spawners(root), files)?,
            }
        }
        Some(_) | None => select(&read(STRUCTURE)?, &read(TABLE)?, &spawners(root), files)?,
    };
    let selection = selection.with_units(units).with_floor(floor);
    if let Selection::Tests(tests) = &selection
        && let Some((unknown, listed)) = tests.unknown_share()
        && unknown > 0
    {
        start_refresh(program, root, unknown, listed);
    }
    Ok(selection)
}

/// Start a refresh when more than a tenth of the tests this lane's build lists are unknown to the
/// map, which [`north test coverage refresh --start`] decides and starts once per repository. The
/// call is quiet when the share is under the tenth.
fn start_refresh(program: &Path, root: &Path, unknown: usize, listed: usize) {
    let (unknown, listed) = (unknown.to_string(), listed.to_string());
    let output = Command::new(program).args(["test", "coverage", "refresh", "--start", "--unknown", &unknown, "--listed", &listed]).current_dir(root).stdin(Stdio::null()).output();
    match output {
        Ok(output) if output.status.success() => {
            let said = String::from_utf8_lossy(&output.stdout);
            let said = said.trim();
            if !said.is_empty() {
                eprintln!("safe-push: {said}");
            }
        }
        Ok(output) => eprintln!("safe-push: north test coverage refresh --start failed: {}", String::from_utf8_lossy(&output.stderr).trim()),
        Err(error) => eprintln!("safe-push: north test coverage refresh --start: {error}"),
    }
}

fn units(table: &str, plan: &Plan) -> Result<Units, String> {
    Units::parse(table, &plan.names().chain([ROOT]).collect())
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

#[cfg(test)]
#[path = "it_modules_tests.rs"]
mod tests;
