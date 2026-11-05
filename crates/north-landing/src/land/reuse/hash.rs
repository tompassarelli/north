use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const SEPARATOR: &str = " :: ";
const SKIPPED: [&str; 2] = ["target", ".git"];
const WORKSPACE_FILES: [&str; 5] = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain", ".cargo/config.toml"];
const SCRIPT_EXTENSIONS: [&str; 6] = ["sh", "bash", "zsh", "mjs", "js", "ts"];
const SCRIPT_DEPTH: usize = 3;
const SCRIPT_LIMIT: u64 = 1 << 20;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    #[serde(default)]
    workspace_default_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Target {
    src_path: PathBuf,
}

#[derive(Default)]
struct Selection {
    packages: BTreeSet<String>,
    workspace: bool,
}

pub fn input_hash(root: &Path, check_line: &str) -> Result<String, String> {
    let root = fs::canonicalize(root).map_err(|error| format!("{}: {error}", root.display()))?;
    let line = check_line.trim();
    let command = line.split_once(SEPARATOR).map_or(line, |(_, command)| command);
    let mut files = BTreeSet::new();
    named_files(&root, command, &mut files);
    let selections = cargo_selections(command);
    if !selections.is_empty() {
        cargo_files(&root, &selections, &mut files)?;
    }
    digest(&root, line, &files)
}

fn digest(root: &Path, line: &str, files: &BTreeSet<PathBuf>) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(format!("check\0{line}\n"));
    for relative in files {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", relative.display()))?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|error| format!("{}: {error}", relative.display()))?;
            hasher.update(format!("link\0{}\0{}\n", relative.display(), target.display()));
        } else {
            let content = fs::read(&path).map_err(|error| format!("{}: {error}", relative.display()))?;
            hasher.update(format!("file\0{}\0{}\n", relative.display(), content.len()));
            hasher.update(&content);
        }
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn collect(root: &Path, relative: &Path, excluded: &[PathBuf], files: &mut BTreeSet<PathBuf>) {
    let Ok(metadata) = fs::symlink_metadata(root.join(relative)) else {
        return;
    };
    if !metadata.is_dir() {
        files.insert(relative.to_owned());
        return;
    }
    let Ok(entries) = fs::read_dir(root.join(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let child = relative.join(entry.file_name());
        let skipped = SKIPPED.iter().any(|name| entry.file_name() == *name);
        if !skipped && !excluded.contains(&child) {
            collect(root, &child, excluded, files);
        }
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || ";&|(),=\"'<>`${}:".contains(c)).filter(|word| !word.is_empty())
}

fn repository_path(word: &str) -> Option<PathBuf> {
    let word = word.strip_prefix("PWD/").unwrap_or(word);
    let path = Path::new(word.trim_start_matches("./"));
    let inside = !path.is_absolute() && path.components().all(|part| matches!(part, Component::Normal(_)));
    let named = path.components().next().is_none_or(|part| SKIPPED.iter().all(|name| part.as_os_str() != *name));
    (inside && named).then(|| path.to_owned())
}

fn is_script(path: &Path, head: &[u8]) -> bool {
    head.starts_with(b"#!") || path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension))
}

fn named_files(root: &Path, text: &str, files: &mut BTreeSet<PathBuf>) {
    let mut level: Vec<String> = words(text).map(str::to_owned).collect();
    for depth in 0..=SCRIPT_DEPTH {
        let mut next = Vec::new();
        for word in &level {
            if let Some(script) = add_named(root, word, files)
                && depth < SCRIPT_DEPTH
            {
                let body = fs::read_to_string(root.join(script)).unwrap_or_default();
                next.extend(words(&body).filter(|word| word.contains('/') || word.contains('.')).map(str::to_owned));
            }
        }
        level = next;
    }
}

fn add_named(root: &Path, word: &str, files: &mut BTreeSet<PathBuf>) -> Option<PathBuf> {
    let path = repository_path(word)?;
    let metadata = fs::symlink_metadata(root.join(&path)).ok()?;
    let fresh = !files.contains(&path);
    let script = fresh && metadata.is_file() && metadata.len() <= SCRIPT_LIMIT && is_script(&path, &fs::read(root.join(&path)).unwrap_or_default());
    collect(root, &path, &[], files);
    script.then_some(path)
}


fn cargo_selections(command: &str) -> Vec<Selection> {
    let mut selections = Vec::new();
    for segment in command.split(['\n', ';', '&', '|']) {
        let mut tokens = segment.split_whitespace().map(|token| token.trim_matches(['"', '\'']));
        if !tokens.any(|token| Path::new(token).file_name().is_some_and(|name| name == "cargo")) {
            continue;
        }
        let mut selection = Selection::default();
        while let Some(token) = tokens.next() {
            match token {
                "-p" | "--package" => selection.packages.extend(tokens.next().map(str::to_owned)),
                "--workspace" | "--all" => selection.workspace = true,
                _ => {
                    if let Some(name) = token.strip_prefix("--package=").or_else(|| token.strip_prefix("-p").filter(|name| !name.starts_with('-'))) {
                        selection.packages.insert(name.to_owned());
                    }
                }
            }
        }
        selections.push(selection);
    }
    selections
}

fn metadata(root: &Path) -> Result<Metadata, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("cargo metadata: {error}"))
}

fn cargo_files(root: &Path, selections: &[Selection], files: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    for name in WORKSPACE_FILES {
        if root.join(name).is_file() {
            files.insert(PathBuf::from(name));
        }
    }
    let metadata = metadata(root)?;
    let members = identifiers(&metadata.workspace_members);
    let defaults = identifiers(&metadata.workspace_default_members);
    let workspace: Vec<&Package> = metadata.packages.iter().filter(|package| members.contains(package.id.as_str())).collect();
    let mut chosen: BTreeSet<&str> = BTreeSet::new();
    for selection in selections {
        for package in &workspace {
            let named = selection.packages.contains(&package.name);
            let default = selection.packages.is_empty() && defaults.contains(package.id.as_str());
            if named || default || selection.workspace {
                chosen.insert(&package.name);
            }
        }
    }
    let edges = internal_edges(&workspace);
    let mut included = reach(&chosen, &edges, false);
    included.extend(reach(&chosen, &edges, true));
    let directories: BTreeMap<&str, PathBuf> = workspace.iter().map(|package| (package.name.as_str(), package_directory(root, package))).collect();
    for package in workspace.iter().filter(|package| included.contains(package.name.as_str())) {
        let directory = &directories[package.name.as_str()];
        if directory.as_os_str().is_empty() {
            root_package_files(root, package, files);
        } else {
            let nested: Vec<PathBuf> = directories.values().filter(|other| other.starts_with(directory) && *other != directory).cloned().collect();
            collect(root, directory, &nested, files);
        }
    }
    Ok(())
}

fn identifiers(ids: &[String]) -> BTreeSet<&str> {
    ids.iter().map(String::as_str).collect()
}

fn internal_edges<'a>(workspace: &[&'a Package]) -> Vec<(&'a str, &'a str)> {
    let names: BTreeSet<&str> = workspace.iter().map(|package| package.name.as_str()).collect();
    workspace
        .iter()
        .flat_map(|package| package.dependencies.iter().filter(|dependency| dependency.path.is_some() && names.contains(dependency.name.as_str())).map(|dependency| (package.name.as_str(), dependency.name.as_str())))
        .collect()
}

fn reach<'a>(start: &BTreeSet<&'a str>, edges: &[(&'a str, &'a str)], reverse: bool) -> BTreeSet<&'a str> {
    let mut seen = start.clone();
    let mut frontier: Vec<&str> = start.iter().copied().collect();
    while let Some(current) = frontier.pop() {
        for &(user, used) in edges {
            let (from, to) = if reverse { (used, user) } else { (user, used) };
            if from == current && seen.insert(to) {
                frontier.push(to);
            }
        }
    }
    seen
}


fn package_directory(root: &Path, package: &Package) -> PathBuf {
    let manifest = package.manifest_path.parent().unwrap_or(Path::new(""));
    manifest.strip_prefix(root).map(Path::to_owned).unwrap_or_default()
}

fn root_package_files(root: &Path, package: &Package, files: &mut BTreeSet<PathBuf>) {
    files.insert(PathBuf::from("Cargo.toml"));
    for target in &package.targets {
        let Ok(source) = target.src_path.strip_prefix(root) else {
            continue;
        };
        match source.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            Some(parent) => collect(root, parent, &[], files),
            None => {
                files.insert(source.to_owned());
            }
        }
    }
}
