use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use north_base::build_inputs::rust_build_input;
use serde::Deserialize;

use super::select::changed;

pub const PACKAGES_ENV: &str = "SAFE_PUSH_LAND_PACKAGES";
pub const CHANGED_ENV: &str = "SAFE_PUSH_LAND_CHANGED";
pub const CPUS_ENV: &str = "SAFE_PUSH_LAND_CPUS";
const WORKSPACE: &str = "--workspace";
const ROOT_SOURCES: [&str; 4] = ["src/", "tests/", "examples/", "benches/"];
const ROOT_FILES: [&str; 1] = ["build.rs"];
const CPUS_PER_PACKAGE: usize = 2;
const MINIMUM_CPUS: usize = 4;
const DEP_INFO: &str = "debug/deps";

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: String,
    #[serde(default)]
    target_directory: Option<String>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    id: String,
    manifest_path: String,
    dependencies: Vec<Dependency>,
    #[serde(default)]
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    name: String,
    #[serde(default)]
    kind: Vec<String>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Workspace,
    Packages { affected: Vec<String>, changed: Vec<String> },
}

impl Scope {
    pub fn arguments(&self) -> String {
        match self {
            Self::Workspace => WORKSPACE.to_owned(),
            Self::Packages { affected, .. } => flags(affected),
        }
    }

    pub fn changed_arguments(&self) -> String {
        match self {
            Self::Workspace => WORKSPACE.to_owned(),
            Self::Packages { changed, .. } => flags(changed),
        }
    }

    pub fn cpus(&self) -> Option<usize> {
        match self {
            Self::Workspace => None,
            Self::Packages { affected, .. } => Some((affected.len() * CPUS_PER_PACKAGE).max(MINIMUM_CPUS)),
        }
    }
}

fn flags(names: &[String]) -> String {
    names.iter().map(|name| format!("-p {name}")).collect::<Vec<_>>().join(" ")
}

struct Member<'a> {
    directory: String,
    package: &'a Package,
}

enum Owner<'a> {
    Everything,
    Package(&'a str),
    Packages(Vec<&'a str>),
    Nothing,
}

pub struct DepInfo {
    pub stem: String,
    pub contents: String,
}

type Embedders<'a> = HashMap<String, BTreeSet<&'a str>>;

pub fn select(metadata: &str, files: &[String], dep_info: &[DepInfo]) -> Result<Scope, String> {
    let metadata: Metadata = serde_json::from_str(metadata).map_err(|error| format!("cargo metadata: {error}"))?;
    let root = Path::new(&metadata.workspace_root);
    let mut members = Vec::new();
    for package in metadata.packages.iter().filter(|package| metadata.workspace_members.contains(&package.id)) {
        let manifest = Path::new(&package.manifest_path);
        let directory = manifest.parent().and_then(|parent| parent.strip_prefix(root).ok()).ok_or_else(|| format!("cargo metadata: {} is outside the workspace root", package.manifest_path))?;
        let directory = directory.to_string_lossy().into_owned();
        members.push(Member { directory: if directory.is_empty() { directory } else { format!("{directory}/") }, package });
    }
    let embedders = embedders(&members, root, dep_info);
    let mut changed = BTreeSet::new();
    for file in files {
        match owner(&members, &embedders, file) {
            Owner::Everything => return Ok(Scope::Workspace),
            Owner::Package(name) => {
                changed.insert(name);
            }
            Owner::Packages(names) => changed.extend(names),
            Owner::Nothing => {}
        }
    }
    let affected = dependents(&members, changed.clone());
    if affected.is_empty() || affected.len() == members.len() || !library(&members, &affected) {
        return Ok(Scope::Workspace);
    }
    Ok(Scope::Packages { affected: affected.into_iter().map(str::to_owned).collect(), changed: changed.into_iter().map(str::to_owned).collect() })
}

fn library(members: &[Member], affected: &BTreeSet<&str>) -> bool {
    members.iter().filter(|member| affected.contains(member.package.name.as_str())).any(|member| member.package.targets.iter().any(|target| target.kind.iter().any(|kind| kind == "lib")))
}

fn owner<'a>(members: &[Member<'a>], embedders: &Embedders<'a>, file: &str) -> Owner<'a> {
    let nested = members.iter().filter(|member| !member.directory.is_empty() && file.starts_with(&member.directory)).max_by_key(|member| member.directory.len());
    if let Some(member) = nested {
        return Owner::Package(&member.package.name);
    }
    if let Some(root) = members.iter().find(|member| member.directory.is_empty())
        && (ROOT_SOURCES.iter().any(|directory| file.starts_with(directory)) || ROOT_FILES.contains(&file))
    {
        return Owner::Package(&root.package.name);
    }
    if let Some(names) = embedders.get(file) {
        return Owner::Packages(names.iter().copied().collect());
    }
    if rust_build_input(file) || ROOT_SOURCES.iter().any(|directory| file.starts_with(directory)) { Owner::Everything } else { Owner::Nothing }
}

fn embedders<'a>(members: &[Member<'a>], root: &Path, dep_info: &[DepInfo]) -> Embedders<'a> {
    let mut targets: HashMap<String, Vec<&str>> = HashMap::new();
    for member in members {
        for target in &member.package.targets {
            targets.entry(target.name.replace('-', "_")).or_default().push(member.package.name.as_str());
        }
    }
    let mut embedders = Embedders::new();
    for info in dep_info {
        let Some(packages) = info.stem.rsplit_once('-').and_then(|(name, _)| targets.get(name)) else { continue };
        for input in dependencies(&info.contents).filter_map(|input| workspace_path(root, &input)) {
            embedders.entry(input).or_default().extend(packages.iter().copied());
        }
    }
    embedders
}

fn dependencies(contents: &str) -> impl Iterator<Item = String> + '_ {
    contents.lines().filter(|line| !line.starts_with('#')).filter_map(|line| line.strip_suffix(':')).filter(|line| !line.contains(": ")).map(|line| line.replace("\\ ", " "))
}

fn workspace_path(root: &Path, input: &str) -> Option<String> {
    let input = Path::new(input);
    let relative = if input.is_absolute() { input.strip_prefix(root).ok()? } else { input };
    let mut normal = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => normal.push(part),
            Component::ParentDir if !normal.pop() => return None,
            Component::CurDir | Component::ParentDir => {}
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    normal.to_str().map(str::to_owned)
}

fn read_dep_info(metadata: &str) -> Vec<DepInfo> {
    let Some(target) = serde_json::from_str::<Metadata>(metadata).ok().and_then(|metadata| metadata.target_directory) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(Path::new(&target).join(DEP_INFO)) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "d"))
        .filter_map(|path| Some(DepInfo { stem: path.file_stem()?.to_str()?.to_owned(), contents: std::fs::read_to_string(&path).ok()? }))
        .collect()
}

fn dependents<'a>(members: &[Member<'a>], changed: BTreeSet<&'a str>) -> BTreeSet<&'a str> {
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    for member in members {
        for dependency in member.package.dependencies.iter().filter(|dependency| dependency.path.is_some()) {
            reverse.entry(dependency.name.as_str()).or_default().push(member.package.name.as_str());
        }
    }
    let mut affected = changed.clone();
    let mut pending: Vec<&str> = changed.into_iter().collect();
    while let Some(name) = pending.pop() {
        for dependent in reverse.get(name).into_iter().flatten() {
            if affected.insert(dependent) {
                pending.push(dependent);
            }
        }
    }
    affected
}

pub fn scope(root: &Path, base: &str, head: &str) -> Result<Scope, String> {
    let metadata = north_base::platform::cargo_metadata::no_deps(root)?;
    select(&metadata, &changed(root, base, head, &[]), &read_dep_info(&metadata))
}

#[cfg(test)]
#[path = "affected_tests.rs"]
mod tests;
