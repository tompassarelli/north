use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Test {
    pub binary: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Function {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub reached_by: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    pub commit: String,
    pub recorded: u64,
    pub tests: Vec<Test>,
    pub functions: Vec<Function>,
    #[serde(default)]
    pub index: Index,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub symbols: Vec<String>,
    pub items: Vec<Item>,
    pub refers: Vec<Refers>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub file: String,
    pub start: u32,
    pub end: u32,
    pub defines: Vec<u32>,
    pub uses: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refers {
    pub function: u32,
    pub symbols: Vec<u32>,
}

impl Map {
    pub fn in_file<'a>(&'a self, file: &'a str) -> impl Iterator<Item = &'a Function> + 'a {
        self.functions.iter().filter(move |function| function.file == file)
    }

    pub fn reaching(&self, file: &str) -> std::collections::BTreeSet<Test> {
        self.in_file(file).flat_map(|function| function.reached_by.iter()).filter_map(|index| self.tests.get(*index as usize).cloned()).collect()
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read the coverage map {}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("cannot parse the coverage map {}: {error}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let directory = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
        std::fs::create_dir_all(directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let text = serde_json::to_string(self).map_err(|error| format!("cannot encode the coverage map: {error}"))?;
        let staged = tempfile::NamedTempFile::new_in(directory).map_err(|error| format!("cannot stage the coverage map in {}: {error}", directory.display()))?;
        std::fs::write(staged.path(), text).map_err(|error| format!("cannot write the coverage map: {error}"))?;
        staged.persist(path).map(|_| ()).map_err(|error| format!("cannot install the coverage map at {}: {error}", path.display()))
    }
}

pub fn state_home() -> PathBuf {
    let home = || std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"));
    std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map(PathBuf::from).or_else(home).unwrap_or_default()
}

pub fn path_for(repository: &str) -> PathBuf {
    state_home().join("north/coverage").join(format!("{repository}.json"))
}
