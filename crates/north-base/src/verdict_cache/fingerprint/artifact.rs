use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Tree;
use super::environment::replace_roots;
use crate::verdict_cache::path_set::Kind;

const ENV_DEP: &str = "# env-dep:";
const OUTSIDE: [&str; 3] = ["/nix/store/", "/rustc/", "/.cargo/registry/"];

struct Package {
    path_source: bool,
    manifest: Option<String>,
    build_script: Option<String>,
    libraries: Vec<String>,
    targets: Vec<String>,
    dependencies: Vec<usize>,
}

#[derive(Default)]
struct DepInfo {
    sources: Vec<String>,
    environment: Vec<String>,
}

pub struct Artifacts {
    root: PathBuf,
    inside: Option<PathBuf>,
    target: PathBuf,
    packages: Vec<Package>,
    answers: HashMap<String, Option<String>>,
}

fn relative_to(root: &Path, text: &str) -> Option<String> {
    Path::new(text).strip_prefix(root).ok().and_then(Path::to_str).map(str::to_owned)
}

fn package_of(root: &Path, value: &Value) -> Package {
    let strings = |value: &Value| value.as_array().map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();
    let targets = value["targets"].as_array().cloned().unwrap_or_default();
    let of_kind = |kind: &str| targets.iter().filter(move |target| strings(&target["kind"]).iter().any(|found| found == kind)).cloned().collect::<Vec<_>>();
    let libraries = ["lib", "rlib", "proc-macro", "dylib", "cdylib", "staticlib"].iter().flat_map(|kind| of_kind(kind)).filter_map(|target| target["name"].as_str().map(|name| name.replace('-', "_"))).collect();
    Package {
        path_source: value["source"].is_null(),
        manifest: value["manifest_path"].as_str().and_then(|path| relative_to(root, path)),
        build_script: of_kind("custom-build").first().and_then(|target| target["src_path"].as_str()).and_then(|path| relative_to(root, path)),
        libraries,
        targets: targets.iter().filter_map(|target| target["name"].as_str().map(|name| name.replace('-', "_"))).collect(),
        dependencies: Vec::new(),
    }
}

fn graph(root: &Path, metadata: &str) -> Option<Vec<Package>> {
    let value: Value = serde_json::from_str(metadata).ok()?;
    let listed = value["packages"].as_array()?;
    let index: HashMap<String, usize> = listed.iter().enumerate().filter_map(|(position, package)| Some((package["id"].as_str()?.to_owned(), position))).collect();
    let mut packages: Vec<Package> = listed.iter().map(|package| package_of(root, package)).collect();
    for node in value["resolve"]["nodes"].as_array()? {
        let Some(from) = node["id"].as_str().and_then(|id| index.get(id)) else { continue };
        packages[*from].dependencies = node["dependencies"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|id| index.get(id).copied()).collect();
    }
    Some(packages)
}

fn rooted(text: &str, root: &Path, inside: Option<&Path>) -> String {
    match (inside.and_then(Path::to_str), root.to_str()) {
        (Some(inside), Some(root)) if inside != root => text.replace(inside, root),
        _ => text.to_owned(),
    }
}

fn split_hash(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('-') {
        Some((stem, hash)) if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) => (stem, Some(hash)),
        _ => (name, None),
    }
}

fn names_library(file: &str, library: &str) -> bool {
    let Some((name, Some(_))) = file.strip_suffix(".d").map(split_hash) else { return false };
    name == library || name.strip_prefix("lib") == Some(library)
}

fn parse_dep_info(text: &str) -> DepInfo {
    let mut info = DepInfo::default();
    if let Some((_, sources)) = text.lines().next().and_then(|line| line.split_once(": ")) {
        info.sources = sources.split(' ').filter(|source| !source.is_empty()).map(str::to_owned).collect();
    }
    info.environment = text.lines().filter_map(|line| line.strip_prefix(ENV_DEP)).map(str::to_owned).collect();
    info
}

impl Artifacts {
    pub fn open(root: &Path, inside: Option<&Path>, target: &Path, metadata: Option<&str>) -> Self {
        let packages = metadata.map(|metadata| rooted(metadata, root, inside)).and_then(|metadata| graph(root, &metadata)).unwrap_or_default();
        Self { root: root.to_owned(), inside: inside.map(Path::to_owned), target: target.to_owned(), packages, answers: HashMap::new() }
    }

    fn dep_info(&self, file: &Path) -> Option<DepInfo> {
        fs::read_to_string(file).ok().map(|text| parse_dep_info(&rooted(&text, &self.root, self.inside.as_deref())))
    }

    fn deps_names(directory: &Path) -> Vec<String> {
        fs::read_dir(directory).into_iter().flatten().filter_map(|entry| entry.ok()?.file_name().into_string().ok()).filter(|name| name.ends_with(".d")).collect()
    }

    fn closure(&self, stem: &str) -> Vec<usize> {
        let stem = stem.replace('-', "_");
        let mut seen: BTreeSet<usize> = self.packages.iter().enumerate().filter(|(_, package)| package.targets.contains(&stem)).map(|(position, _)| position).collect();
        let mut pending: Vec<usize> = seen.iter().copied().collect();
        while let Some(next) = pending.pop() {
            for dependency in &self.packages[next].dependencies {
                if self.packages[*dependency].path_source && seen.insert(*dependency) {
                    pending.push(*dependency);
                }
            }
        }
        seen.into_iter().collect()
    }

    fn build_identity(&self, artifact: &Path, hash: Option<&str>) -> Option<String> {
        if let Some(hash) = hash {
            return Some(hash.to_owned());
        }
        let inode = fs::metadata(artifact).ok()?;
        let deps = artifact.parent()?.join("deps");
        let name = artifact.file_name()?.to_str()?;
        fs::read_dir(deps).ok()?.filter_map(Result::ok).find_map(|entry| {
            let file = entry.file_name().into_string().ok()?;
            let (stem, hash) = split_hash(&file);
            let same = stem == name.replace('-', "_") || stem == name;
            let linked = same && fs::metadata(entry.path()).is_ok_and(|other| other.ino() == inode.ino() && other.dev() == inode.dev());
            hash.filter(|_| linked).map(str::to_owned)
        })
    }

    fn source_answer(&self, tree: &Tree, source: &str) -> Option<String> {
        let source = source.strip_prefix("./").unwrap_or(source);
        let relative = if source.starts_with('/') {
            if let Some(inside) = relative_to(&self.root, source) {
                inside
            } else if OUTSIDE.iter().any(|marker| source.starts_with(marker) || source.contains(marker)) {
                return Some(format!("{source} = outside the tree"));
            } else {
                return None;
            }
        } else {
            source.to_owned()
        };
        let target = relative_to(&self.root, self.target.to_str()?).unwrap_or_default();
        if !target.is_empty() && (relative == target || relative.starts_with(&format!("{target}/"))) {
            return Some(format!("{relative} = generated under target"));
        }
        Some(format!("{relative} = {}", tree.answer(Kind::Content, &relative)?))
    }

    fn compute(&self, tree: &Tree, relative: &str) -> Option<String> {
        if self.packages.is_empty() {
            return None;
        }
        let artifact = self.target.join(relative);
        let name = artifact.file_name()?.to_str()?;
        let directory = artifact.parent()?;
        let (stem, hash) = split_hash(name);
        let identity = self.build_identity(&artifact, hash)?;
        let deps = if directory.ends_with("deps") { directory.to_owned() } else { directory.join("deps") };
        let mut files = vec![directory.join(format!("{name}.d"))];
        let mut extra: BTreeSet<String> = BTreeSet::new();
        for package in self.closure(stem).into_iter().map(|position| &self.packages[position]) {
            extra.extend(package.manifest.iter().chain(package.build_script.iter()).cloned());
            for library in &package.libraries {
                files.extend(Self::deps_names(&deps).into_iter().filter(|file| names_library(file, library)).map(|file| deps.join(file)));
            }
        }
        let mut sources: BTreeSet<String> = extra;
        let mut environment: BTreeSet<String> = BTreeSet::new();
        for (position, file) in files.iter().enumerate() {
            let Some(info) = self.dep_info(file) else {
                if position == 0 {
                    return None;
                }
                continue;
            };
            sources.extend(info.sources);
            environment.extend(info.environment.into_iter().map(|value| replace_roots(&value, &[(&self.target, "<target>"), (&self.root, "<tree>")])));
        }
        let mut hasher = Sha256::new();
        hasher.update(format!("artifact\0{stem}\0{identity}\n"));
        for value in &environment {
            hasher.update(format!("env-dep\0{value}\n"));
        }
        for source in &sources {
            hasher.update(format!("source\0{}\n", self.source_answer(tree, source)?));
        }
        Some(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
    }

    pub fn answer(&mut self, tree: &Tree, relative: &str) -> Option<String> {
        if let Some(found) = self.answers.get(relative) {
            return found.clone();
        }
        let answer = self.compute(tree, relative);
        self.answers.insert(relative.to_owned(), answer.clone());
        answer
    }
}
