use std::collections::BTreeSet;
use std::path::Path;

use toml_edit::{DocumentMut, Item};

use super::select::git_lines;

pub const TABLE: &str = "harness/guard-suites.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Suite {
    name: String,
    paths: Vec<String>,
    tests: BTreeSet<(String, Option<String>)>,
    it: BTreeSet<String>,
    cases: BTreeSet<(String, String)>,
}

impl Suite {
    fn guards(&self, file: &str) -> bool {
        self.paths.iter().any(|path| if path.ends_with('/') { file.starts_with(path.as_str()) } else { file == path })
    }

    fn left(&self, from: &str, to: Option<&str>, added: &[String]) -> Vec<String> {
        match to {
            Some(to) if !self.guards(to) => vec![format!("{from} moved to {to}, outside every path of the {} suite", self.name)],
            Some(_) => Vec::new(),
            None => added.iter().filter(|file| file.ends_with(".rs") && !self.guards(file)).map(|file| format!("{from} was deleted while {file} was added outside every path of the {} suite", self.name)).collect(),
        }
    }
}

pub struct Suites(Vec<Suite>);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Floor {
    names: BTreeSet<String>,
    tests: BTreeSet<(String, Option<String>)>,
    cases: BTreeSet<(String, String)>,
    pub it: BTreeSet<String>,
}

impl Suites {
    pub fn parse(text: &str) -> Result<Self, String> {
        let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| format!("{TABLE}: {error}"))?;
        let tables = document.get("suite").and_then(Item::as_array_of_tables).ok_or_else(|| format!("{TABLE}: no [[suite]] tables"))?;
        let mut suites = Vec::new();
        for table in tables {
            let name = table.get("name").and_then(Item::as_str).ok_or_else(|| format!("{TABLE}: a [[suite]] has no name"))?.to_owned();
            let list = |key: &str| -> Result<Vec<String>, String> {
                let array = table.get(key).and_then(Item::as_array).ok_or_else(|| format!("{TABLE}: suite {name} {key} must be a list of strings"))?;
                array.iter().map(|value| value.as_str().map(str::to_owned).ok_or_else(|| format!("{TABLE}: suite {name} {key} must be a list of strings"))).collect()
            };
            let paths = list("paths")?;
            if paths.is_empty() {
                return Err(format!("{TABLE}: suite {name} guards no path"));
            }
            let tests = list("tests")?.iter().map(|entry| entry.split_once(' ').map_or((entry.clone(), None), |(package, module)| (package.to_owned(), Some(module.to_owned())))).collect();
            let cases = match table.get("cases") {
                Some(item) => item
                    .as_array()
                    .ok_or_else(|| format!("{TABLE}: suite {name} cases must be a list of strings"))?
                    .iter()
                    .map(|value| {
                        let entry = value.as_str().ok_or_else(|| format!("{TABLE}: suite {name} cases must be a list of strings"))?;
                        let (binary, test) = entry.split_once(' ').ok_or_else(|| format!("{TABLE}: suite {name} case {entry} is not BINARY TEST"))?;
                        (!binary.is_empty() && !test.is_empty()).then(|| (binary.to_owned(), test.to_owned())).ok_or_else(|| format!("{TABLE}: suite {name} case {entry} is not BINARY TEST"))
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?,
                None => BTreeSet::new(),
            };
            suites.push(Suite { tests, cases, it: list("it")?.into_iter().collect(), paths, name });
        }
        Ok(Self(suites))
    }

    pub fn read(root: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(root.join(TABLE)) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self(Vec::new())),
            Err(error) => Err(format!("{TABLE}: {error}")),
        }
    }

    pub fn floor(&self, files: &[String]) -> Floor {
        let mut floor = Floor::default();
        for suite in self.0.iter().filter(|suite| files.iter().any(|file| suite.guards(file))) {
            floor.names.insert(suite.name.clone());
            floor.tests.extend(suite.tests.iter().cloned());
            floor.cases.extend(suite.cases.iter().cloned());
            floor.it.extend(suite.it.iter().cloned());
        }
        floor
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().flat_map(|suite| &suite.paths).map(String::as_str)
    }

    pub fn missing(&self, root: &Path) -> Vec<String> {
        self.0.iter().flat_map(|suite| &suite.paths).filter(|path| !root.join(path.as_str()).exists()).cloned().collect()
    }

    pub fn moved_out(&self, renames: &[(String, Option<String>)], added: &[String]) -> Vec<String> {
        self.0.iter().flat_map(|suite| renames.iter().filter(|(from, _)| suite.guards(from)).flat_map(move |(from, to)| suite.left(from, to.as_deref(), added))).collect()
    }
}

impl Floor {
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn packages(&self) -> BTreeSet<String> {
        self.tests
            .iter()
            .map(|(package, _)| package.clone())
            .chain(self.cases.iter().map(|(binary, _)| binary.split("::").next().unwrap_or(binary).to_owned()))
            .collect()
    }

    pub fn names(&self) -> String {
        self.names.iter().cloned().collect::<Vec<_>>().join(", ")
    }

    pub fn has_it_case(&self) -> bool {
        self.cases.iter().any(|(binary, _)| binary.ends_with("::it"))
    }

    pub fn selects(&self, name: &str) -> bool {
        let module = self.tests.iter().filter_map(|(_, module)| module.as_deref()).any(|module| {
            let module = module.trim_end_matches(':');
            name == module || name.starts_with(&format!("{module}::")) || name.contains(&format!("::{module}::"))
        });
        let case = self.cases.iter().any(|(_, test)| name == test || name.ends_with(&format!("::{test}")));
        module || case
    }

    pub fn entries(&self) -> BTreeSet<String> {
        self.tests.iter().map(|(package, module)| module.as_ref().map_or_else(|| package.clone(), |module| format!("{package} {module}"))).collect()
    }

    pub fn filter(&self) -> String {
        let tests = self.tests.iter().map(|(package, module)| module.as_ref().map_or_else(|| format!("package({package})"), |module| format!("(package({package})&test(/^{module}/))")));
        let cases = self.cases.iter().map(|(binary, test)| format!("(binary_id({binary})&test(={test}))"));
        tests.chain(cases).collect::<Vec<_>>().join("|")
    }
}

pub fn refuse(root: &Path, base: &str, head: &str, out: &mut impl std::io::Write) -> bool {
    let suites = match Suites::read(root) {
        Ok(suites) => suites,
        Err(error) => {
            let _ = writeln!(out, "guard suite: {error}");
            return true;
        }
    };
    let mut renames = Vec::new();
    let mut added = Vec::new();
    for line in git_lines(root, &["diff", "-M", "--name-status", "--no-color", base, head]) {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[..] {
            [status, from, to] if status.starts_with('R') => {
                renames.push((from.to_owned(), Some(to.to_owned())));
                added.push(to.to_owned());
            }
            [status, path] if status.starts_with('D') => renames.push((path.to_owned(), None)),
            [status, path] if status.starts_with('A') => added.push(path.to_owned()),
            _ => {}
        }
    }
    let problems: Vec<String> = suites.missing(root).into_iter().map(|path| format!("{TABLE} guards {path}, which does not exist at the lane's head")).chain(suites.moved_out(&renames, &added)).collect();
    for problem in &problems {
        let _ = writeln!(out, "guard suite: {problem}; list the new home under that suite's paths in {TABLE} so its tests keep guarding the code");
    }
    !problems.is_empty()
}

#[cfg(test)]
#[path = "guard_suites_tests.rs"]
mod tests;
