use std::collections::BTreeSet;

use north_coverage::hunks::Hunks;
use north_coverage::pick::{Touched, touched_regions};

use crate::facts::{Facts, TestId};
use crate::lane::{Change, Changed, Status};
use crate::workspace::{self, Member, Workspace};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub package: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cone {
    pub tests: BTreeSet<TestId>,
    pub always: BTreeSet<TestId>,
    pub added: Vec<Added>,
    pub files: Vec<String>,
    pub untraced: Option<String>,
}

impl Cone {
    pub fn holds(&self, binary: Option<&str>, name: &str) -> bool {
        let traced = self.tests.iter().any(|test| test.name == name && binary.is_none_or(|binary| binary == test.binary));
        let in_package = |package: &str| package.is_empty() || binary.is_none_or(|binary| binary.split("::").next() == Some(package));
        let added = self.added.iter().any(|test| (name == test.name || name.ends_with(&format!("::{}", test.name))) && in_package(&test.package));
        self.untraced.is_some() || traced || added
    }
}

pub fn untraced_input(path: &str) -> bool {
    const INPUTS: [&str; 5] = ["rust-toolchain", "rust-toolchain.toml", "flake.nix", "flake.lock", "bin/cargo"];
    INPUTS.contains(&path) || path.starts_with(".cargo/") || path == ".config/nextest.toml"
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(directory, _)| directory)
}

struct Walk<'f, 'g> {
    facts: &'f Facts<'g>,
    stale: Option<&'f Hunks>,
    tests: BTreeSet<TestId>,
    fallback: BTreeSet<String>,
}

impl Walk<'_, '_> {
    fn crate_fallback(&mut self, path: &str) -> Result<(), String> {
        if !self.facts.knows_crates() {
            return Err(format!("{path} needs the crate fallback and the graph holds no crate facts"));
        }
        let name = self.facts.crate_of(path).ok_or_else(|| format!("no crate in the graph holds {path}"))?;
        self.fallback.insert(name.to_owned());
        Ok(())
    }

    fn rust(&mut self, path: &str, lane: Option<&[north_coverage::hunks::Hunk]>) -> Result<(), String> {
        let regions = self.facts.regions(path)?;
        let Some(stale) = self.stale.filter(|_| !regions.is_empty()) else {
            return self.crate_fallback(path);
        };
        let touched = match lane {
            Some(hunks) => touched_regions(&regions.iter().map(|region| (region.start, region.end)).collect::<Vec<_>>(), stale.get(path).map_or(&[][..], Vec::as_slice), hunks),
            None => Touched::Outside,
        };
        match touched {
            Touched::Regions(indices) => {
                for index in indices {
                    self.tests.extend(self.facts.reaching(&regions[index].node)?);
                }
            }
            Touched::Outside => self.tests.extend(self.facts.reaching_file(path)?),
        }
        Ok(())
    }
}

pub fn of(facts: &Facts, change: &Change, stale: Option<&Hunks>) -> Result<Cone, String> {
    let files: Vec<String> = change.files.iter().map(|changed| changed.path.clone()).collect();
    let added = change.added.iter().map(|test| Added { package: test.package.clone().or_else(|| facts.crate_of(&test.file).map(str::to_owned)).unwrap_or_default(), name: test.name.clone() }).collect();
    let mut cone = Cone { added, files, ..Cone::default() };
    match walk(facts, change, stale) {
        Ok(tests) => cone.tests = tests,
        Err(reason) => cone.untraced = Some(reason),
    }
    if cone.untraced.is_none() && change.files.iter().any(|changed| !changed.path.ends_with(".rs")) {
        cone.always = facts.unrecorded()?.difference(&cone.tests).cloned().collect();
    }
    Ok(cone)
}

fn workspace_fallback(facts: &Facts, workspace: &Workspace) -> Result<BTreeSet<String>, String> {
    if let Some(why) = &workspace.untraced {
        return Err(why.clone());
    }
    if !workspace.members.is_empty() && !facts.knows_crates() {
        return Err("a workspace member changed and the graph holds no crate facts for its fallback".to_owned());
    }
    Ok(workspace
        .members
        .iter()
        .filter_map(|member| match member {
            Member::Package(name) => Some(name.clone()),
            Member::Directory(directory) => facts.crate_rooted_at(directory).map(str::to_owned),
        })
        .collect())
}

impl Walk<'_, '_> {
    fn path(&mut self, changed: &Changed, change: &Change, embedded: &mut Vec<String>) -> Result<(), String> {
        let (facts, path) = (self.facts, changed.path.as_str());
        let mut read = facts.readers("reads", path)?;
        if changed.status != Status::Modified {
            read.extend(facts.readers("lists", parent(path))?);
        }
        if changed.status == Status::Added {
            read.extend(facts.readers("expects-absent", path)?);
        }
        let embedders = facts.embedders(path)?;
        let manifest = facts.manifest_of(path).filter(|_| !workspace::input(path)).map(str::to_owned);
        let traced = !read.is_empty() || !embedders.is_empty() || manifest.is_some();
        self.tests.extend(read);
        embedded.extend(embedders);
        if let Some(name) = manifest {
            self.fallback.insert(name);
        } else if path.ends_with(".rs") {
            self.rust(path, Some(change.hunks.get(path).map_or(&[][..], Vec::as_slice)))?;
        } else if !traced && !facts.reads_recorded && !workspace::input(path) {
            return Err(format!("{path} is not Rust and the graph holds no recorded reads"));
        }
        Ok(())
    }
}

fn walk(facts: &Facts, change: &Change, stale: Option<&Hunks>) -> Result<BTreeSet<TestId>, String> {
    if let Some(input) = change.files.iter().find(|changed| untraced_input(&changed.path)) {
        return Err(format!("{} is a toolchain input the graph does not trace", input.path));
    }
    let mut walk = Walk { facts, stale, tests: BTreeSet::new(), fallback: workspace_fallback(facts, &change.workspace)? };
    let mut seen: BTreeSet<String> = change.files.iter().map(|changed| changed.path.clone()).collect();
    let mut embedded: Vec<String> = Vec::new();
    for changed in &change.files {
        walk.path(changed, change, &mut embedded)?;
    }
    let mut whole: Vec<String> = embedded.into_iter().filter(|source| seen.insert(source.clone())).collect();
    while let Some(source) = whole.pop() {
        walk.tests.extend(facts.readers("reads", &source)?);
        whole.extend(facts.embedders(&source)?.into_iter().filter(|embedder| seen.insert(embedder.clone())));
        if source.ends_with(".rs") {
            walk.rust(&source, None)?;
        }
    }
    let packages: BTreeSet<String> = walk.fallback.iter().flat_map(|name| facts.dependents(name)).collect();
    walk.tests.extend(facts.tests_of(&packages));
    Ok(walk.tests)
}
