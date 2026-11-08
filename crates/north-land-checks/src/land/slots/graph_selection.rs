use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

const IT_BINARY: &str = "north::it";
const IT_PACKAGE: &str = "north";
const BIN_MARK: &str = "::bin/";
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
    known: usize,
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
        let tests = Self { by_binary, added, modules: BTreeSet::new(), known: answer.known };
        (!tests.by_binary.is_empty() || !tests.added.is_empty()).then_some(tests).filter(|tests| tests.filter().len() <= FILTER_LIMIT)
    }

    pub fn with_modules(self, modules: BTreeSet<String>) -> Self {
        Self { modules, ..self }
    }

    pub fn runs_module(&self, module: &str) -> bool {
        self.modules.contains(module) || self.by_binary.get(IT_BINARY).is_some_and(|names| names.iter().any(|name| name.strip_prefix(module).is_some_and(|rest| rest.starts_with("::"))))
    }

    pub fn packages(&self) -> BTreeSet<String> {
        let binaries = self.by_binary.keys().map(|binary| binary.split("::").next().unwrap_or(binary).to_owned());
        let modules = (!self.modules.is_empty()).then(|| IT_PACKAGE.to_owned());
        binaries.chain(self.added.iter().map(|(package, _)| package.clone()).filter(|package| !package.is_empty())).chain(modules).collect()
    }

    pub fn arguments(&self) -> String {
        let it = !self.modules.is_empty() || self.by_binary.contains_key(IT_BINARY) || self.added.iter().any(|(package, _)| package == IT_PACKAGE);
        format!("{}-E {}", if it { "--test it " } else { "" }, self.filter())
    }

    pub fn summary(&self) -> String {
        let traced: usize = self.by_binary.values().map(BTreeSet::len).sum();
        format!("the {traced} of the {} tests the change graph knows, in {} test binaries, and the {} tests the lane adds, and the tree-wide tests/it modules {}", self.known, self.by_binary.len(), self.added.len(), self.modules.iter().cloned().collect::<Vec<_>>().join(", "))
    }

    fn filter(&self) -> String {
        let traced = self.by_binary.iter().map(|(binary, names)| format!("(binary_id({binary})&({}))", names.iter().map(|name| format!("test(={name})")).collect::<Vec<_>>().join("|")));
        let added = self.added.iter().map(|(package, name)| if package.is_empty() { format!("test(/(^|::){name}$/)") } else { format!("(package({package})&test(/(^|::){name}$/))") });
        let modules = (!self.modules.is_empty()).then(|| format!("(binary_id({IT_BINARY})&test(/^({})::/))", self.modules.iter().cloned().collect::<Vec<_>>().join("|")));
        traced.chain(added).chain(modules).collect::<Vec<_>>().join("|")
    }
}

fn built(binary: &str) -> bool {
    !binary.contains("::") || binary.contains(BIN_MARK) || binary == IT_BINARY
}

pub fn ask(program: &Path, root: &Path, base: &str, head: &str) -> Option<Tests> {
    if base.is_empty() {
        return None;
    }
    let output = Command::new(program).args(["graph", "ask", "affected", "--json", "--repo"]).arg(root).args([base, head]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| Tests::parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}
