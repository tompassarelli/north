use std::collections::BTreeSet;

use north_coverage::hunks::{Hunk, Hunks};
use north_coverage::pick::{Probe, innermost, locate};

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proof {
    pub tests: BTreeSet<TestId>,
    pub untraced: Option<String>,
}

impl Cone {
    pub fn holds(&self, binary: Option<&str>, name: &str) -> bool {
        let traced = self.tests.iter().any(|test| test.name == name && binary.is_none_or(|binary| binary == test.binary));
        let in_package = |package: &str| package.is_empty() || binary.is_none_or(|binary| binary.split("::").next() == Some(package));
        let added = self.added.iter().any(|test| (name == test.name || name.ends_with(&format!("::{}", test.name))) && in_package(&test.package));
        self.untraced.is_some() || traced || added
    }

    pub fn nextest_arguments(&self) -> Vec<String> {
        let tests = self.tests.iter().chain(&self.always);
        let packages: BTreeSet<&str> = tests.clone().map(TestId::package).chain(self.added.iter().map(|test| test.package.as_str()).filter(|package| !package.is_empty())).collect();
        let traced = tests.map(|test| format!("(binary_id({})&test(={}))", test.binary, test.name));
        let added = self.added.iter().map(|test| if test.package.is_empty() { format!("test(/(^|::){}$/)", test.name) } else { format!("(package({})&test(/(^|::){}$/))", test.package, test.name) });
        let filter = traced.chain(added).collect::<Vec<_>>().join("|");
        if filter.is_empty() {
            return Vec::new();
        }
        packages.into_iter().flat_map(|package| ["-p".to_owned(), package.to_owned()]).chain(["-E".to_owned(), filter]).collect()
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
    proof: bool,
    tests: BTreeSet<TestId>,
    fallback: BTreeSet<String>,
    bare: BTreeSet<String>,
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

    fn rust(&mut self, path: &str, lane: Option<&[Hunk]>) -> Result<(), String> {
        let regions = self.facts.regions(path)?;
        let Some(stale) = self.stale.filter(|_| !regions.is_empty()) else {
            return self.crate_fallback(path);
        };
        let Some(hunks) = lane else {
            self.tests.extend(self.facts.reaching_file(path)?);
            return Ok(());
        };
        let stale = stale.get(path).map_or(&[][..], Vec::as_slice);
        let located = locate(&regions.iter().map(|region| (region.start, region.end)).collect::<Vec<_>>(), stale, hunks);
        for index in located.regions {
            self.tests.extend(self.facts.reaching(&regions[index].node)?);
        }
        match (located.outside.is_empty(), self.proof) {
            (true, _) => Ok(()),
            (false, false) => {
                self.tests.extend(self.facts.reaching_file(path)?);
                Ok(())
            }
            (false, true) => self.outside(path, stale, &located.outside),
        }
    }

    fn outside(&mut self, path: &str, stale: &[Hunk], probes: &[Probe]) -> Result<(), String> {
        let items = self.facts.items(path)?;
        let spans: Vec<(u32, u32)> = items.iter().map(|item| (item.start, item.end)).collect();
        for probe in probes {
            let Some(index) = innermost(&spans, stale, *probe) else {
                return self.crate_fallback(path);
            };
            match self.facts.referring(&items[index].node)? {
                Some(tests) => self.tests.extend(tests),
                None => self.tests.extend(self.facts.reaching_file(path)?),
            }
        }
        Ok(())
    }
}

pub fn of(facts: &Facts, change: &Change, stale: Option<&Hunks>) -> Result<Cone, String> {
    let files: Vec<String> = change.files.iter().map(|changed| changed.path.clone()).collect();
    let added = change.added.iter().map(|test| Added { package: test.package.clone().or_else(|| facts.crate_of(&test.file).map(str::to_owned)).unwrap_or_default(), name: test.name.clone() }).collect();
    let mut cone = Cone { added, files, ..Cone::default() };
    let unknown = unknown(facts, change, &cone.added);
    cone.added.extend(unknown);
    match walk(facts, change, stale, false) {
        Ok((tests, bare)) => {
            cone.tests = tests;
            cone.added.extend(bare.into_iter().map(|name| Added { package: String::new(), name }));
        }
        Err(reason) => cone.untraced = Some(reason),
    }
    if cone.untraced.is_none() && change.files.iter().any(|changed| !changed.path.ends_with(".rs")) {
        cone.always = facts.unrecorded()?.difference(&cone.tests).cloned().collect();
    }
    Ok(cone)
}

pub fn since_base(root: &std::path::Path, facts: &Facts, base: &str, cone: &Cone) -> Result<BTreeSet<TestId>, String> {
    let Some(hunks) = facts.coverage_commit.as_deref().filter(|_| cone.untraced.is_none()).and_then(|commit| crate::lane::rust_since(root, commit, base)) else { return Ok(BTreeSet::new()) };
    Ok(since_map(facts, &hunks)?.into_iter().filter(|test| !cone.tests.contains(test) && !cone.always.contains(test)).collect())
}

pub fn since_map(facts: &Facts, hunks: &Hunks) -> Result<BTreeSet<TestId>, String> {
    let mut tests = BTreeSet::new();
    for (path, changes) in hunks {
        let regions = facts.regions(path)?;
        if regions.is_empty() {
            continue;
        }
        let located = locate(&regions.iter().map(|region| (region.start, region.end)).collect::<Vec<_>>(), &[], changes);
        for index in located.regions {
            tests.extend(facts.reaching(&regions[index].node)?);
        }
        if !located.outside.is_empty() {
            tests.extend(facts.reaching_file(path)?);
        }
    }
    Ok(tests)
}

fn unknown(facts: &Facts, change: &Change, added: &[Added]) -> Vec<Added> {
    let reached: BTreeSet<String> = change.files.iter().filter(|changed| changed.path.ends_with(".rs") && !change.apart.contains(&changed.path)).filter_map(|changed| facts.crate_of(&changed.path)).flat_map(|name| facts.dependents(name)).collect();
    let known: BTreeSet<(String, String)> = facts.tests_of(&reached).into_iter().map(|id| (id.package().to_owned(), id.name.rsplit("::").next().unwrap_or(&id.name).to_owned())).collect();
    let mut found: Vec<Added> = Vec::new();
    for test in &change.present {
        let Some(package) = facts.crate_of(&test.file).filter(|package| reached.contains(*package)) else { continue };
        let entry = Added { package: package.to_owned(), name: test.name.clone() };
        if !known.contains(&(entry.package.clone(), entry.name.clone())) && !added.contains(&entry) && !found.contains(&entry) {
            found.push(entry);
        }
    }
    found
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
        let (guarded, bare) = facts.guarded(path)?;
        read.extend(guarded);
        let traced_bare = !bare.is_empty();
        self.bare.extend(bare);
        if changed.status != Status::Modified {
            read.extend(facts.readers("lists", parent(path))?);
        }
        if changed.status == Status::Added {
            read.extend(facts.readers("expects-absent", path)?);
        }
        let embedders = facts.embedders(path)?;
        let manifest = facts.manifest_of(path).filter(|_| !workspace::input(path)).map(str::to_owned);
        let traced = traced_bare || !read.is_empty() || !embedders.is_empty() || manifest.is_some();
        self.tests.extend(read);
        embedded.extend(embedders);
        if let Some(name) = manifest {
            self.fallback.insert(name);
        } else if path.ends_with(".rs") {
            if facts.reads_recorded && change.apart.contains(path) {
                return Ok(());
            }
            self.rust(path, Some(change.hunks.get(path).map_or(&[][..], Vec::as_slice)))?;
        } else if !traced && !facts.reads_recorded && !workspace::input(path) {
            return Err(format!("{path} is not Rust and the graph holds no recorded reads"));
        }
        Ok(())
    }
}

pub fn proof(facts: &Facts, change: &Change, stale: Option<&Hunks>) -> Proof {
    match walk(facts, change, stale, true) {
        Ok((tests, _)) => Proof { tests, untraced: None },
        Err(reason) => Proof { tests: BTreeSet::new(), untraced: Some(reason) },
    }
}

fn walk(facts: &Facts, change: &Change, stale: Option<&Hunks>, proof: bool) -> Result<(BTreeSet<TestId>, BTreeSet<String>), String> {
    if let Some(input) = change.files.iter().find(|changed| untraced_input(&changed.path)) {
        return Err(format!("{} is a toolchain input the graph does not trace", input.path));
    }
    let mut walk = Walk { facts, stale, proof, tests: BTreeSet::new(), bare: BTreeSet::new(), fallback: workspace_fallback(facts, &change.workspace)? };
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
    Ok((walk.tests, walk.bare))
}
