use std::path::Path;
use std::process::Command;

use toml_edit::{DocumentMut, Item};

use super::Shared;
use super::attribution::Repository;

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// A lane's repository as attribution reads it: paths in its checkout, test definitions at its head and manifests at its base,
/// so a lane cannot widen or narrow the packages a red test exercises by editing a manifest.
pub(super) struct LaneTree<'a> {
    root: &'a Path,
    base: &'a str,
    head: &'a str,
}

impl Shared {
    pub(super) fn lane_tree(&self) -> LaneTree<'_> {
        LaneTree { root: &self.root, base: &self.arguments.base, head: &self.arguments.head }
    }
}

impl LaneTree<'_> {
    fn git(&self, arguments: &[&str]) -> Option<String> {
        let output = Command::new("git").args(arguments).current_dir(self.root).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Repository for LaneTree<'_> {
    fn exists(&self, path: &str) -> bool {
        self.root.join(path).exists()
    }

    fn test_files(&self, name: &str) -> Vec<String> {
        let Some(leaf) = name.rsplit("::").next().filter(|leaf| !leaf.is_empty()) else { return Vec::new() };
        let text = self.git(&["grep", "-l", "-E", &format!(r"fn {leaf}\b"), self.head, "--", "*.rs"]).unwrap_or_default();
        text.lines().map(|line| line.split_once(':').map_or(line, |(_, path)| path).to_owned()).collect()
    }

    fn path_dependencies(&self, package: &str) -> Option<Vec<String>> {
        manifest_dependencies(package, &self.git(&["show", &format!("{}:{package}Cargo.toml", self.base)])?)
    }
}

/// The packages `manifest`, the Cargo.toml of the package in directory `package`, depends on by path, each as its directory
/// with a trailing slash and the root package as ""; `None` when the manifest does not parse.
pub(super) fn manifest_dependencies(package: &str, manifest: &str) -> Option<Vec<String>> {
    let document = manifest.parse::<DocumentMut>().ok()?;
    let paths = DEPENDENCY_TABLES.iter().filter_map(|table| document.get(table).and_then(Item::as_table_like)).flat_map(|table| table.iter().filter_map(|(_, dependency)| dependency.get("path").and_then(Item::as_str).map(str::to_owned)).collect::<Vec<_>>());
    Some(paths.map(|path| joined(package, &path)).collect())
}

fn joined(package: &str, path: &str) -> String {
    let mut parts: Vec<&str> = package.split('/').filter(|part| !part.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() { String::new() } else { format!("{}/", parts.join("/")) }
}
