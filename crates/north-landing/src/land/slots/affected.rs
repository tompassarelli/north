use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;

use north_base::build_inputs::rust_build_input;
use serde::Deserialize;

use super::select::changed;

pub const PACKAGES_ENV: &str = "SAFE_PUSH_LAND_PACKAGES";
pub const CPUS_ENV: &str = "SAFE_PUSH_LAND_CPUS";
const WORKSPACE: &str = "--workspace";
const ROOT_SOURCES: [&str; 4] = ["src/", "tests/", "examples/", "benches/"];
const ROOT_FILES: [&str; 1] = ["build.rs"];
const CPUS_PER_PACKAGE: usize = 2;
const MINIMUM_CPUS: usize = 4;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: String,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    id: String,
    manifest_path: String,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Workspace,
    Packages(Vec<String>),
}

impl Scope {
    pub fn arguments(&self) -> String {
        match self {
            Self::Workspace => WORKSPACE.to_owned(),
            Self::Packages(names) => names.iter().map(|name| format!("-p {name}")).collect::<Vec<_>>().join(" "),
        }
    }

    pub fn cpus(&self) -> Option<usize> {
        match self {
            Self::Workspace => None,
            Self::Packages(names) => Some((names.len() * CPUS_PER_PACKAGE).max(MINIMUM_CPUS)),
        }
    }
}

struct Member<'a> {
    directory: String,
    package: &'a Package,
}

enum Owner<'a> {
    Everything,
    Package(&'a str),
    Nothing,
}

pub fn select(metadata: &str, files: &[String]) -> Result<Scope, String> {
    let metadata: Metadata = serde_json::from_str(metadata).map_err(|error| format!("cargo metadata: {error}"))?;
    let root = Path::new(&metadata.workspace_root);
    let mut members = Vec::new();
    for package in metadata.packages.iter().filter(|package| metadata.workspace_members.contains(&package.id)) {
        let manifest = Path::new(&package.manifest_path);
        let directory = manifest.parent().and_then(|parent| parent.strip_prefix(root).ok()).ok_or_else(|| format!("cargo metadata: {} is outside the workspace root", package.manifest_path))?;
        let directory = directory.to_string_lossy().into_owned();
        members.push(Member { directory: if directory.is_empty() { directory } else { format!("{directory}/") }, package });
    }
    let mut changed = BTreeSet::new();
    for file in files {
        match owner(&members, file) {
            Owner::Everything => return Ok(Scope::Workspace),
            Owner::Package(name) => {
                changed.insert(name);
            }
            Owner::Nothing => {}
        }
    }
    let affected = dependents(&members, changed);
    if affected.is_empty() || affected.len() == members.len() {
        return Ok(Scope::Workspace);
    }
    Ok(Scope::Packages(affected.into_iter().map(str::to_owned).collect()))
}

fn owner<'a>(members: &[Member<'a>], file: &str) -> Owner<'a> {
    let nested = members.iter().filter(|member| !member.directory.is_empty() && file.starts_with(&member.directory)).max_by_key(|member| member.directory.len());
    if let Some(member) = nested {
        return Owner::Package(&member.package.name);
    }
    if let Some(root) = members.iter().find(|member| member.directory.is_empty())
        && (ROOT_SOURCES.iter().any(|directory| file.starts_with(directory)) || ROOT_FILES.contains(&file))
    {
        return Owner::Package(&root.package.name);
    }
    if rust_build_input(file) || ROOT_SOURCES.iter().any(|directory| file.starts_with(directory)) { Owner::Everything } else { Owner::Nothing }
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
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--offline"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata exited {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }
    select(&String::from_utf8_lossy(&output.stdout), &changed(root, base, head, &[]))
}

#[cfg(test)]
#[path = "affected_tests.rs"]
mod tests;
