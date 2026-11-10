use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::guard_suites::Floor;
use super::tree_wide_units::Units;

const IT_BINARY: &str = "north::it";
const IT_PACKAGE: &str = "north";
const FILTER_LIMIT: usize = 96 * 1024;

#[derive(Deserialize)]
struct Answer {
    tests: Vec<String>,
    #[serde(default)]
    always: Vec<String>,
    #[serde(default)]
    added: Vec<AddedTest>,
    #[serde(default)]
    known: usize,
    #[serde(default)]
    carried: Vec<String>,
    #[serde(default)]
    since: Vec<String>,
}

#[derive(Deserialize)]
struct AddedTest {
    #[serde(default)]
    package: String,
    name: String,
}

/// The tests a `cargo nextest list` run found in one binary at the lane's head.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    pub package: String,
    pub tests: BTreeSet<String>,
}

pub type ListedTests = BTreeMap<String, Listed>;

#[derive(Deserialize)]
struct Listing {
    #[serde(rename = "rust-suites", default)]
    suites: BTreeMap<String, Suite>,
}

#[derive(Deserialize)]
struct Suite {
    #[serde(rename = "binary-id")]
    binary_id: String,
    #[serde(rename = "package-name", default)]
    package: String,
    #[serde(default)]
    testcases: BTreeMap<String, serde_json::Value>,
}

/// Every test `cargo nextest list` finds at `root`, by binary id. An unbuilt or unlistable tree lists nothing, so the selection falls back to the graph answer alone.
pub fn list(root: &Path) -> ListedTests {
    if !root.join("Cargo.toml").is_file() {
        return BTreeMap::new();
    }
    let output = Command::new("cargo").args(["nextest", "list", "--message-format", "json"]).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output();
    match output {
        Ok(output) if output.status.success() => listing(&String::from_utf8_lossy(&output.stdout)),
        _ => BTreeMap::new(),
    }
}

fn listing(json: &str) -> ListedTests {
    let Ok(listing) = serde_json::from_str::<Listing>(json) else { return BTreeMap::new() };
    listing.suites.into_values().map(|suite| (suite.binary_id, Listed { package: suite.package, tests: suite.testcases.into_keys().collect() })).filter(|(_, listed)| !listed.tests.is_empty()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tests {
    by_binary: BTreeMap<String, BTreeSet<String>>,
    added: BTreeSet<(String, String)>,
    modules: BTreeSet<String>,
    units: Units,
    floor: Floor,
    known: usize,
    carried: usize,
    listed: BTreeMap<String, Listed>,
    since: BTreeSet<String>,
}

impl Tests {
    pub fn parse(json: &str) -> Option<Self> {
        let answer: Answer = serde_json::from_str(json).ok()?;
        let mut by_binary: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in answer.tests.iter().chain(&answer.always) {
            let (binary, name) = id.split_once(' ')?;
            by_binary.entry(binary.to_owned()).or_default().insert(name.to_owned());
        }
        let added = answer.added.into_iter().map(|test| (test.package, test.name)).collect();
        let tests = Self { by_binary, added, modules: BTreeSet::new(), units: Units::default(), floor: Floor::default(), known: answer.known, carried: answer.carried.len(), listed: BTreeMap::new(), since: answer.since.into_iter().collect() };
        (!tests.by_binary.is_empty() || !tests.added.is_empty()).then_some(tests)
    }

    /// The inclusion filter's second source: every test `cargo nextest list` sees that the map does not know. `added` already names the unknown tests the change graph scanned; the listing gives each its binary, and it catches a test the scan cannot see. A test the listing does not match stays a name term. `None` when the resulting filter passes the limit, so the caller runs the whole closure instead of fewer tests.
    pub fn with_listed(mut self, listed: ListedTests) -> Option<Self> {
        let listed: ListedTests = listed.into_iter().filter(|(_, suite)| !suite.tests.is_empty()).collect();
        if !listed.is_empty() {
            let unknown = self.added.clone();
            let matches: Vec<(String, String)> = listed
                .iter()
                .flat_map(|(binary, suite)| {
                    unknown.iter().filter(|(package, _)| package.is_empty() || *package == suite.package || binary.split("::").next() == Some(package.as_str())).flat_map(move |(_, name)| {
                        suite.tests.iter().filter(move |test| test.as_str() == name || test.ends_with(&format!("::{name}"))).map(move |test| (binary.clone(), test.clone()))
                    })
                })
                .collect();
            for (binary, test) in matches {
                self.by_binary.entry(binary).or_default().insert(test);
            }
            self.by_binary.retain(|binary, names| !names.is_empty() && listed.contains_key(binary));
            self.listed = listed;
        }
        (self.filter().len() <= FILTER_LIMIT).then_some(self)
    }

    pub fn with_modules(self, modules: BTreeSet<String>) -> Self {
        Self { modules, ..self }
    }

    pub fn with_units(self, units: Units) -> Self {
        Self { units, ..self }
    }

    pub fn with_floor(self, floor: Floor) -> Self {
        let modules = self.modules.iter().chain(&floor.it).cloned().collect();
        Self { modules, floor, ..self }
    }

    pub fn runs_module(&self, module: &str) -> bool {
        self.modules.contains(module) || self.by_binary.get(IT_BINARY).is_some_and(|names| names.iter().any(|name| name.strip_prefix(module).is_some_and(|rest| rest.starts_with("::"))))
    }

    pub fn packages(&self) -> BTreeSet<String> {
        let binaries = self.by_binary.keys().map(|binary| binary.split("::").next().unwrap_or(binary).to_owned());
        let modules = (!self.modules.is_empty()).then(|| IT_PACKAGE.to_owned());
        binaries.chain(self.added.iter().map(|(package, _)| package.clone()).filter(|package| !package.is_empty())).chain(modules).chain(self.units.packages()).chain(self.floor.packages()).collect()
    }

    pub fn selected(&self) -> Selected {
        let tests = self.by_binary.values().flatten().cloned().chain(self.units.tests().map(str::to_owned)).collect();
        Selected { tests, added: self.added.iter().map(|(_, name)| name.clone()).collect(), modules: self.modules.clone(), floor: self.floor.entries(), every: false }
    }

    pub fn arguments(&self) -> String {
        format!("--tests -E {}", self.filter())
    }

    pub fn unselected_since(&self) -> Vec<String> {
        let selected = self.selected();
        self.since.iter().filter(|id| id.split_once(' ').is_none_or(|(binary, name)| !selected.holds(name) && (binary != IT_BINARY || !self.runs_module(name.split("::").next().unwrap_or(name))))).cloned().collect()
    }

    pub fn summary(&self) -> String {
        let traced: usize = self.by_binary.values().map(BTreeSet::len).sum();
        format!("the {traced} of the {} tests the change graph knows, in {} test binaries, less the {} the landed lanes' alone passes ran that no other lane can affect, and the {} tests the lane adds, and the tree-wide tests/it modules {}, and the tree-wide unit tests {}, and the guard suites {}", self.known, self.by_binary.len(), self.carried, self.added.len(), self.modules.iter().cloned().collect::<Vec<_>>().join(", "), self.units.names(), self.floor.names())
    }

    fn filter(&self) -> String {
        let whole = self.wholly_included();
        let traced = self.by_binary.iter().map(|(binary, names)| {
            if self.listed.get(binary).is_some_and(|listed| whole.packages.contains(&listed.package)) {
                return None;
            }
            if whole.binaries.contains(binary) {
                return Some(format!("binary_id({binary})"));
            }
            Some(format!("(binary_id({binary})&({}))", names.iter().map(|name| format!("test(={name})")).collect::<Vec<_>>().join("|")))
        });
        let packages = whole.packages.iter().map(|package| format!("package({package})"));
        let added = self.added.iter().map(|(package, name)| if package.is_empty() { format!("test(/(^|::){name}$/)") } else { format!("(package({package})&test(/(^|::){name}$/))") });
        let modules = (!self.modules.is_empty()).then(|| format!("(binary_id({IT_BINARY})&test(/^({})::/))", self.modules.iter().cloned().collect::<Vec<_>>().join("|")));
        let traced: Vec<String> = packages.map(Some).chain(traced).flatten().collect();
        traced.into_iter().chain(added).chain(modules).chain((!self.units.is_empty()).then(|| self.units.filter())).chain((!self.floor.is_empty()).then(|| self.floor.filter())).collect::<Vec<_>>().join("|")
    }

    /// The packages and binaries whose every listed test the selection holds, so one `package` or `binary_id` term names them instead of one term per test. Only a binary `cargo nextest list` saw can be whole; a package collapses only when every listed binary it has is.
    fn wholly_included(&self) -> Wholly {
        let mut packages: BTreeSet<String> = BTreeSet::new();
        let mut binaries: BTreeSet<String> = BTreeSet::new();
        let mut by_package: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
        for (binary, listed) in &self.listed {
            by_package.entry(listed.package.as_str()).or_default().push(binary);
        }
        for (package, members) in by_package {
            if members.iter().all(|binary| self.listed.get(*binary).is_some_and(|listed| self.by_binary.get(*binary).is_some_and(|names| names.is_superset(&listed.tests)))) {
                packages.insert(package.to_owned());
                binaries.extend(members.into_iter().cloned());
            }
        }
        for (binary, listed) in &self.listed {
            if !binaries.contains(binary) && self.by_binary.get(binary).is_some_and(|names| names.is_superset(&listed.tests)) {
                binaries.insert(binary.clone());
            }
        }
        Wholly { packages, binaries }
    }
}

pub const SELECTED: &str = "selected";

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selected {
    pub tests: BTreeSet<String>,
    pub added: BTreeSet<String>,
    pub modules: BTreeSet<String>,
    #[serde(default)]
    pub floor: BTreeSet<String>,
    #[serde(default)]
    pub every: bool,
}

impl Selected {
    pub fn holds(&self, name: &str) -> bool {
        let added = |added: &String| name == added || name.ends_with(&format!("::{added}"));
        let floor = |entry: &String| entry.split_once(' ').is_some_and(|(_, module)| name.starts_with(module));
        self.every || self.tests.contains(name) || self.floor.iter().any(floor) || self.added.iter().any(added) || self.modules.iter().any(|module| name.starts_with(&format!("{module}::")))
    }
}

struct Wholly {
    packages: BTreeSet<String>,
    binaries: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    pub lanes: Vec<String>,
    pub ran: Vec<String>,
}

impl Batch {
    fn arguments(&self) -> impl Iterator<Item = &str> {
        let lanes = self.lanes.iter().flat_map(|lane| ["--lane", lane.as_str()]);
        lanes.chain(self.ran.iter().flat_map(|pass| ["--ran", pass.as_str()]))
    }
}

pub fn ask(program: &Path, root: &Path, base: &str, head: &str, batch: &Batch) -> Option<Tests> {
    if base.is_empty() {
        return None;
    }
    let output = Command::new(program).args(["graph", "ask", "affected", "--json", "--repo"]).arg(root).args(batch.arguments()).args([base, head]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| Tests::parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}
