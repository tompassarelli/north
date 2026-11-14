use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

pub struct Package {
    pub name: String,
    pub label: String,
    pub tests: Vec<TestTarget>,
}

pub struct TestTarget {
    pub target: String,
    root: PathBuf,
}

impl TestTarget {
    fn new(target: String, root: PathBuf) -> TestTarget {
        TestTarget { target, root }
    }

    pub fn binary(&self) -> String {
        format!("{}-bin", self.target)
    }

    pub fn mentions(&self, probe: &str) -> bool {
        mentions(&self.root, probe)
    }
}

pub struct Workspace {
    pub packages: BTreeMap<String, Package>,
}

impl Workspace {
    pub fn read(root: &Path) -> Result<Workspace, String> {
        let manifest = read_manifest(&root.join("Cargo.toml"))?;
        let mut members: Vec<String> = manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("members"))
            .and_then(|members| members.as_array())
            .ok_or_else(|| format!("{}: no [workspace] members", root.join("Cargo.toml").display()))?
            .iter()
            .filter_map(|member| member.as_str().map(str::to_owned))
            .collect();
        if manifest.get("package").is_some() {
            members.insert(0, String::new());
        }
        let mut packages = BTreeMap::new();
        for member in members {
            let package = package(root, &member)?;
            packages.insert(package.name.clone(), package);
        }
        Ok(Workspace { packages })
    }

    pub fn package(&self, name: &str) -> Result<&Package, String> {
        self.packages.get(name).ok_or_else(|| format!("no workspace package is named {name}"))
    }
}

fn read_manifest(path: &Path) -> Result<DocumentMut, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.parse::<DocumentMut>().map_err(|error| format!("{}: {error}", path.display()))
}

fn package(root: &Path, member: &str) -> Result<Package, String> {
    let directory = root.join(member);
    let manifest = read_manifest(&directory.join("Cargo.toml"))?;
    let name = manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str())
        .ok_or_else(|| format!("{}: no package name", directory.join("Cargo.toml").display()))?
        .to_owned();
    let label = format!("//{member}");
    let mut tests = Vec::new();
    if directory.join("src/lib.rs").is_file() {
        tests.push(TestTarget::new(format!("{label}:{name}-unittest"), directory.join("src")));
    }
    for (binary, path) in binaries(&manifest, &name) {
        let source = directory.join(&path);
        if !source.is_file() {
            continue;
        }
        let root = if source.file_name().is_some_and(|file| file == "main.rs") { source.parent().map_or(source.clone(), Path::to_path_buf) } else { source };
        tests.push(TestTarget::new(format!("{label}:{binary}-bin-unittest"), root));
    }
    let mut files: Vec<(String, PathBuf)> = fs::read_dir(directory.join("tests"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.to_owned();
            let test = if path.is_dir() { path.join("main.rs").is_file() } else { path.extension().is_some_and(|extension| extension == "rs") };
            test.then_some((stem, path))
        })
        .collect();
    files.sort();
    tests.extend(files.into_iter().map(|(file, path)| TestTarget::new(format!("{label}:{name}-test-{file}"), path)));
    Ok(Package { name, label, tests })
}

fn binaries(manifest: &DocumentMut, package: &str) -> Vec<(String, String)> {
    let declared: Vec<(String, String)> = manifest
        .get("bin")
        .and_then(|bins| bins.as_array_of_tables())
        .into_iter()
        .flatten()
        .filter_map(|bin| Some((bin.get("name")?.as_str()?.to_owned(), bin.get("path")?.as_str()?.to_owned())))
        .collect();
    if declared.is_empty() { vec![(package.to_owned(), "src/main.rs".to_owned())] } else { declared }
}

fn mentions(path: &Path, probe: &str) -> bool {
    if path.is_dir() {
        return fs::read_dir(path).into_iter().flatten().flatten().any(|entry| mentions(&entry.path(), probe));
    }
    path.extension().is_some_and(|extension| extension == "rs") && fs::read_to_string(path).is_ok_and(|text| text.contains(probe))
}
