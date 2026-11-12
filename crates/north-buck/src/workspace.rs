use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<RawPackage>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct RawPackage {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    targets: Vec<RawTarget>,
    features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    authors: Vec<String>,
    description: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    rust_version: Option<String>,
}

#[derive(Deserialize)]
struct RawTarget {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
    edition: String,
    #[serde(default = "yes")]
    test: bool,
    #[serde(default = "yes")]
    harness: bool,
    #[serde(rename = "required-features", default)]
    required_features: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
    features: Vec<String>,
}

#[derive(Deserialize)]
struct NodeDep {
    name: String,
    pkg: String,
    dep_kinds: Vec<DepKindInfo>,
}

#[derive(Deserialize)]
struct DepKindInfo {
    kind: Option<String>,
}

pub struct Workspace {
    pub root: PathBuf,
    pub packages: Vec<Package>,
}

pub struct Package {
    pub name: String,
    pub dir: String,
    pub version: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub rust_version: Option<String>,
    pub targets: Vec<Target>,
    pub build_script: Option<String>,
    pub features: Vec<String>,
    pub declared_features: Vec<String>,
    pub deps: Vec<Dep>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Kind {
    Lib,
    ProcMacro,
    Bin,
    Test,
    Example,
}

pub struct Target {
    pub name: String,
    pub kind: Kind,
    pub src_path: String,
    pub edition: String,
    pub test: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

pub struct Dep {
    pub extern_name: String,
    pub package: String,
    pub first_party_dir: Option<String>,
    pub lib_crate: String,
    pub kinds: BTreeSet<DepKind>,
}

pub fn parse(json: &str) -> Result<Workspace, String> {
    let metadata: Metadata = serde_json::from_str(json).map_err(|why| format!("cargo metadata: {why}"))?;
    let members: BTreeSet<&str> = metadata.workspace_members.iter().map(String::as_str).collect();
    let by_id: BTreeMap<&str, &RawPackage> = metadata.packages.iter().map(|package| (package.id.as_str(), package)).collect();
    let nodes: BTreeMap<&str, &Node> = metadata.resolve.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let root = metadata.workspace_root.clone();
    let mut packages = Vec::new();
    for raw in metadata.packages.iter().filter(|package| members.contains(package.id.as_str())) {
        let node = nodes.get(raw.id.as_str()).ok_or_else(|| format!("{}: no resolve node", raw.name))?;
        let dir = relative_dir(&root, &raw.manifest_path)?;
        let (scripts, targets): (Vec<&RawTarget>, Vec<&RawTarget>) = raw.targets.iter().partition(|target| target.kind.iter().any(|kind| kind == "custom-build"));
        let build_script = scripts.first().map(|script| script.src_path.strip_prefix(&root).unwrap_or(&script.src_path).to_string_lossy().into_owned());
        let targets = targets.into_iter().map(|target| convert_target(&root, &raw.name, target)).collect::<Result<Vec<_>, _>>()?;
        let deps = node.deps.iter().map(|dep| convert_dep(&root, &members, &by_id, &raw.name, dep)).filter_map(Result::transpose).collect::<Result<Vec<_>, _>>()?;
        packages.push(Package {
            name: raw.name.clone(),
            dir,
            version: raw.version.clone(),
            authors: raw.authors.clone(),
            description: raw.description.clone(),
            license: raw.license.clone(),
            repository: raw.repository.clone(),
            homepage: raw.homepage.clone(),
            rust_version: raw.rust_version.clone(),
            targets,
            build_script,
            features: node.features.clone(),
            declared_features: raw.features.keys().cloned().collect(),
            deps,
        });
    }
    packages.sort_by(|left, right| left.dir.cmp(&right.dir));
    Ok(Workspace { root, packages })
}

fn relative_dir(root: &Path, manifest: &Path) -> Result<String, String> {
    let dir = manifest.parent().and_then(|dir| dir.strip_prefix(root).ok()).ok_or_else(|| format!("{} is outside the workspace root", manifest.display()))?;
    Ok(dir.to_string_lossy().into_owned())
}

fn convert_target(root: &Path, package: &str, raw: &RawTarget) -> Result<Target, String> {
    let kind = match raw.kind.first().map(String::as_str) {
        Some("lib" | "rlib") => Kind::Lib,
        Some("proc-macro") => Kind::ProcMacro,
        Some("bin") => Kind::Bin,
        Some("test") => Kind::Test,
        Some("example") => Kind::Example,
        other => return Err(format!("{package}: target {} has kind {other:?}, which north buck targets does not carry over", raw.name)),
    };
    if !raw.harness || !raw.required_features.is_empty() {
        return Err(format!("{package}: target {} sets harness = false or required-features, which north buck targets does not carry over", raw.name));
    }
    let src_path = raw.src_path.strip_prefix(root).map_err(|_| format!("{package}: {} is outside the workspace root", raw.src_path.display()))?;
    Ok(Target { name: raw.name.clone(), kind, src_path: src_path.to_string_lossy().into_owned(), edition: raw.edition.clone(), test: raw.test })
}

fn convert_dep(root: &Path, members: &BTreeSet<&str>, by_id: &BTreeMap<&str, &RawPackage>, package: &str, raw: &NodeDep) -> Result<Option<Dep>, String> {
    let mut kinds = BTreeSet::new();
    for info in &raw.dep_kinds {
        match info.kind.as_deref() {
            None => kinds.insert(DepKind::Normal),
            Some("dev") => kinds.insert(DepKind::Dev),
            Some("build") => kinds.insert(DepKind::Build),
            Some(other) => return Err(format!("{package}: dependency {} has kind {other}, which north buck targets does not know", raw.name)),
        };
    }
    let target = by_id.get(raw.pkg.as_str()).ok_or_else(|| format!("{package}: dependency {} is not in cargo metadata", raw.pkg))?;
    let Some(lib) = target.targets.iter().find(|target| target.kind.iter().any(|kind| matches!(kind.as_str(), "lib" | "rlib" | "proc-macro"))) else {
        return Ok(None);
    };
    let first_party_dir = if members.contains(raw.pkg.as_str()) { Some(relative_dir(root, &target.manifest_path)?) } else { None };
    Ok(Some(Dep { extern_name: raw.name.clone(), package: target.name.clone(), first_party_dir, lib_crate: lib.name.replace('-', "_"), kinds }))
}
