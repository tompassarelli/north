use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::scan;
use crate::workspace::{Package, Target};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Export {
    File(String),
    Dir(String),
}

pub struct Owners {
    dirs: Vec<String>,
}

impl Owners {
    pub fn new(dirs: impl Iterator<Item = String>) -> Self {
        let mut dirs: Vec<String> = dirs.collect();
        dirs.sort_by_key(|dir| std::cmp::Reverse(dir.len()));
        Self { dirs }
    }

    fn owner(&self, path: &str) -> &str {
        self.dirs.iter().find(|dir| under(path, dir)).map_or("", String::as_str)
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub prefix: Option<String>,
    pub package_dir: String,
    pub globs: Vec<String>,
    pub extras: Vec<String>,
    pub foreign: BTreeMap<String, String>,
    pub reads: BTreeSet<String>,
}

impl Tree {
    pub fn path_of(&self, repo_path: &str) -> String {
        match &self.prefix {
            Some(_) => repo_path.to_owned(),
            None => relative(repo_path, &self.package_dir).to_owned(),
        }
    }
}

enum Reference {
    File(String),
    Dir(String),
}

pub fn tree(root: &Path, package: &Package, target: &Target, owners: &Owners, exports: &mut BTreeMap<String, BTreeSet<Export>>) -> Result<Tree, String> {
    let local = relative(&target.src_path, &package.dir);
    let glob_root = local.split_once('/').map_or(local, |(first, _)| first);
    let covered = join(&package.dir, glob_root);
    let mut reads = BTreeSet::new();
    let mut references = Vec::new();
    for file in rust_files(&root.join(&covered), &covered)? {
        let text = std::fs::read_to_string(root.join(&file)).map_err(|why| format!("{file}: {why}"))?;
        let found = scan::reads(&text);
        let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
        for path in found.includes.iter().filter(|path| !path.starts_with('/')) {
            references.push(Reference::File(normalize(&join(dir, path)).ok_or_else(|| format!("{file}: {path} leaves the repository"))?));
        }
        for path in &found.manifest_includes {
            references.push(Reference::File(normalize(&format!("{}{path}", package.dir)).ok_or_else(|| format!("{file}: {path} leaves the repository"))?));
        }
        for path in &found.module_paths {
            let module = normalize(&join(dir, path)).ok_or_else(|| format!("{file}: {path} leaves the repository"))?;
            references.push(Reference::Dir(module.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_owned())));
        }
        reads.extend(found.env);
    }
    let globs = if covered == target.src_path { vec![glob_root.to_owned()] } else { vec![format!("{glob_root}/**")] };
    let mut tree = Tree { prefix: None, package_dir: package.dir.clone(), globs, extras: Vec::new(), foreign: BTreeMap::new(), reads };
    let outside: Vec<Reference> = references.into_iter().filter(|reference| !under(reference.path(), &covered) && reference.path() != covered).collect();
    if outside.iter().any(|reference| !under(reference.path(), &package.dir)) {
        tree.prefix = Some(package.dir.clone());
    }
    for reference in outside {
        place(root, &mut tree, reference, owners, exports)?;
    }
    tree.extras.sort();
    tree.extras.dedup();
    Ok(tree)
}

impl Reference {
    fn path(&self) -> &str {
        match self {
            Self::File(path) | Self::Dir(path) => path,
        }
    }
}

fn place(root: &Path, tree: &mut Tree, reference: Reference, owners: &Owners, exports: &mut BTreeMap<String, BTreeSet<Export>>) -> Result<(), String> {
    let path = reference.path().to_owned();
    if !root.join(&path).exists() {
        return Err(format!("{}: reads {path}, which does not exist", tree.package_dir));
    }
    let owner = owners.owner(&path).to_owned();
    let inside = relative(&path, &owner).to_owned();
    if owner == tree.package_dir {
        match reference {
            Reference::File(_) => tree.extras.push(inside),
            Reference::Dir(_) => tree.globs.push(format!("{inside}/**")),
        }
        return Ok(());
    }
    let export = match reference {
        Reference::File(_) => Export::File(inside.clone()),
        Reference::Dir(_) => Export::Dir(inside.clone()),
    };
    exports.entry(owner.clone()).or_default().insert(export);
    tree.foreign.insert(format!("//{owner}:{inside}"), tree.path_of(&path));
    Ok(())
}

fn rust_files(dir: &Path, repo_path: &str) -> Result<Vec<String>, String> {
    if dir.is_file() {
        return Ok(vec![repo_path.to_owned()]);
    }
    let mut files = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|why| format!("{repo_path}: {why}"))?.collect::<Result<_, _>>().map_err(|why| format!("{repo_path}: {why}"))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{repo_path}/{name}");
        if entry.path().is_dir() {
            files.extend(rust_files(&entry.path(), &path)?);
        } else if name.ends_with(".rs") {
            files.push(path);
        }
    }
    Ok(files)
}

fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

fn relative<'a>(path: &'a str, dir: &str) -> &'a str {
    if dir.is_empty() { path } else { path.strip_prefix(dir).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path) }
}

fn join(dir: &str, path: &str) -> String {
    if dir.is_empty() { path.to_owned() } else { format!("{dir}/{path}") }
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}
