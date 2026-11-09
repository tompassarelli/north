use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::tree_wide_units::Units;

const IT_BINARY: &str = "north::it";
const IT_PACKAGE: &str = "north";
const BIN_MARK: &str = "::bin/";
const IT_SUFFIX: &str = "::it";
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
}

#[derive(Deserialize)]
struct AddedTest {
    #[serde(default)]
    package: String,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tests {
    by_binary: BTreeMap<String, BTreeSet<String>>,
    added: BTreeSet<(String, String)>,
    modules: BTreeSet<String>,
    units: Units,
    known: usize,
    carried: usize,
}

impl Tests {
    pub fn parse(json: &str) -> Option<Self> {
        let answer: Answer = serde_json::from_str(json).ok()?;
        let mut by_binary: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in answer.tests.iter().chain(&answer.always) {
            let (binary, name) = id.split_once(' ')?;
            if built(binary) {
                by_binary.entry(binary.to_owned()).or_default().insert(name.to_owned());
            }
        }
        let added = answer.added.into_iter().map(|test| (test.package, test.name)).collect();
        let tests = Self { by_binary, added, modules: BTreeSet::new(), units: Units::default(), known: answer.known, carried: answer.carried.len() };
        (!tests.by_binary.is_empty() || !tests.added.is_empty()).then_some(tests).filter(|tests| tests.filter().len() <= FILTER_LIMIT)
    }

    pub fn with_modules(self, modules: BTreeSet<String>) -> Self {
        Self { modules, ..self }
    }

    pub fn with_units(self, units: Units) -> Self {
        Self { units, ..self }
    }

    pub fn runs_module(&self, module: &str) -> bool {
        self.modules.contains(module) || self.by_binary.get(IT_BINARY).is_some_and(|names| names.iter().any(|name| name.strip_prefix(module).is_some_and(|rest| rest.starts_with("::"))))
    }

    pub fn packages(&self) -> BTreeSet<String> {
        let binaries = self.by_binary.keys().map(|binary| binary.split("::").next().unwrap_or(binary).to_owned());
        let modules = (!self.modules.is_empty()).then(|| IT_PACKAGE.to_owned());
        binaries.chain(self.added.iter().map(|(package, _)| package.clone()).filter(|package| !package.is_empty())).chain(modules).chain(self.units.packages()).collect()
    }

    pub fn selected(&self) -> Selected {
        let tests = self.by_binary.values().flatten().cloned().chain(self.units.tests().map(str::to_owned)).collect();
        Selected { tests, added: self.added.iter().map(|(_, name)| name.clone()).collect(), modules: self.modules.clone() }
    }

    pub fn arguments(&self) -> String {
        let it = !self.modules.is_empty() || self.by_binary.keys().any(|binary| binary.ends_with(IT_SUFFIX)) || self.added.iter().any(|(package, _)| package == IT_PACKAGE);
        format!("{}-E {}", if it { "--test it " } else { "" }, self.filter())
    }

    pub fn summary(&self) -> String {
        let traced: usize = self.by_binary.values().map(BTreeSet::len).sum();
        format!("the {traced} of the {} tests the change graph knows, in {} test binaries, less the {} the landed lanes' alone passes ran that no other lane can affect, and the {} tests the lane adds, and the tree-wide tests/it modules {}, and the tree-wide unit tests {}", self.known, self.by_binary.len(), self.carried, self.added.len(), self.modules.iter().cloned().collect::<Vec<_>>().join(", "), self.units.names())
    }

    fn filter(&self) -> String {
        let traced = self.by_binary.iter().map(|(binary, names)| format!("(binary_id({binary})&({}))", names.iter().map(|name| format!("test(={name})")).collect::<Vec<_>>().join("|")));
        let added = self.added.iter().map(|(package, name)| if package.is_empty() { format!("test(/(^|::){name}$/)") } else { format!("(package({package})&test(/(^|::){name}$/))") });
        let modules = (!self.modules.is_empty()).then(|| format!("(binary_id({IT_BINARY})&test(/^({})::/))", self.modules.iter().cloned().collect::<Vec<_>>().join("|")));
        traced.chain(added).chain(modules).chain((!self.units.is_empty()).then(|| self.units.filter())).collect::<Vec<_>>().join("|")
    }
}

pub const SELECTED: &str = "selected";

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selected {
    pub tests: BTreeSet<String>,
    pub added: BTreeSet<String>,
    pub modules: BTreeSet<String>,
}

impl Selected {
    pub fn holds(&self, name: &str) -> bool {
        let added = |added: &String| name == added || name.ends_with(&format!("::{added}"));
        self.tests.contains(name) || self.added.iter().any(added) || self.modules.iter().any(|module| name.starts_with(&format!("{module}::")))
    }
}

fn built(binary: &str) -> bool {
    !binary.contains("::") || binary.contains(BIN_MARK) || binary.ends_with(IT_SUFFIX)
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
