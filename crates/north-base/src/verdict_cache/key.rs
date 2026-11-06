use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::build_inputs::rust_build_input_paths;

const SEPARATOR: &str = " :: ";
const RUST_INPUT: &str = "@rust-input";
const TOOLCHAIN: [&str; 3] = ["flake.lock", "rust-toolchain.toml", "rust-toolchain"];
const LINE_KEYS: [&str; 3] = ["land.check", "land.after", "land.queue"];
const MACHINE_MARKERS: [&str; 3] = ["code/south", "code/nixos-config", "machine-bound"];
const SCRIPT_LIMIT: usize = 1 << 20;
const BUILDS_NORTH: [&str; 2] = ["bin/head-north", "cargo "];
pub const NORTH_READS: [&str; 4] = ["SAFE_PUSH_LAND_BASE", "SAFE_PUSH_LAND_HEAD", "SAFE_PUSH_LAND_RUNTIME_HEAD", "SAFE_PUSH_LAND_SELECT_PATHS"];
pub const NORTH_IGNORES: [(&str, &str); 4] = [
    ("SAFE_PUSH_LAND_PACKAGES", "a check line expands it in its own text"),
    ("SAFE_PUSH_LAND_CHANGED", "a check line expands it in its own text"),
    ("SAFE_PUSH_LAND_IT", "a check line expands it in its own text"),
    ("SAFE_PUSH_LAND_CPUS", "it sizes the check's lease, never its verdict"),
];
const RUNS_NORTH: [&str; 2] = ["bin/head-north", "north "];
const NO_BUILD: &str = " --no-build ";
pub const HEAD_NORTH_DEFERRED: &str = "head-north: not run before the push";

pub struct Check<'a> {
    pub repo: &'a Path,
    pub tree: &'a str,
    pub command: &'a str,
    pub runtime: &'a str,
    pub environment: &'a [(String, String)],
    pub machine_repos: &'a [(&'a str, &'a Path)],
}

fn git(repo: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

pub fn machine_bound(command: &str) -> bool {
    MACHINE_MARKERS.iter().any(|marker| command.contains(marker))
}

fn selectors(repo: &Path, tree: &str, command: &str) -> Option<BTreeSet<String>> {
    let blob = format!("{tree}:.safe-push");
    let lines = LINE_KEYS.iter().filter_map(|key| git(repo, &["config", "--blob", &blob, "--get-all", key])).collect::<Vec<_>>().join("\n");
    let patterns: BTreeSet<String> = lines
        .lines()
        .filter_map(|line| line.split_once(SEPARATOR))
        .filter(|(_, line_command)| *line_command == command)
        .flat_map(|(paths, _)| paths.split(','))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .flat_map(|path| if path == RUST_INPUT { rust_build_input_paths().map(str::to_owned).collect() } else { vec![path.to_owned()] })
        .collect();
    (!patterns.is_empty()).then_some(patterns)
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || ";&|(),=\"'<>`${}:".contains(c)).filter(|word| !word.is_empty())
}

fn repository_path(word: &str) -> Option<String> {
    let word = word.strip_prefix("PWD/").unwrap_or(word).trim_start_matches("./").trim_end_matches('/');
    let path = Path::new(word);
    let inside = !word.is_empty() && !path.is_absolute() && path.components().all(|part| matches!(part, Component::Normal(_)));
    (inside && (word.contains('/') || word.contains('.'))).then(|| word.to_owned())
}

fn named(repo: &Path, tree: &str, command: &str) -> (BTreeSet<String>, String) {
    let mut paths: BTreeSet<String> = words(command).filter_map(repository_path).collect();
    let mut text = command.to_owned();
    let scripts: Vec<String> = paths.iter().filter(|path| path.starts_with("bin/") || path.ends_with(".sh")).cloned().collect();
    for script in scripts {
        let body = git(repo, &["cat-file", "blob", &format!("{tree}:{script}")]).unwrap_or_default();
        if body.len() <= SCRIPT_LIMIT {
            paths.extend(words(&body).filter_map(repository_path));
            text.push('\n');
            text.push_str(&body);
        }
    }
    (paths, text)
}

fn reads_variable(command: &str, text: &str, name: &str) -> bool {
    text.contains(name) || (RUNS_NORTH.iter().any(|marker| command.contains(marker)) && !NORTH_IGNORES.iter().any(|(ignored, _)| *ignored == name))
}

pub fn built_equivalent(command: &str) -> Option<String> {
    command.contains(NO_BUILD).then(|| command.replacen(NO_BUILD, " ", 1))
}

fn object_specs(tree: &str, paths: &BTreeSet<String>) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            let path = path.trim_end_matches('/');
            if path.contains(['*', '?', '[']) || path.is_empty() { format!("{tree}^{{tree}}") } else { format!("{tree}:{path}") }
        })
        .collect()
}

fn objects(repo: &Path, specs: &[String]) -> Option<String> {
    let mut child = Command::new("git").arg("-C").arg(repo).args(["cat-file", "--batch-check=%(objectname)"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut input = child.stdin.take()?;
    let request: String = specs.iter().map(|spec| format!("{spec}\n")).collect();
    let writer = std::thread::spawn(move || input.write_all(request.as_bytes()));
    let output = child.wait_with_output().ok()?;
    writer.join().ok()?.ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn environment_value(repo: &Path, value: &str) -> String {
    let resolved = (!value.is_empty() && !value.starts_with('-')).then(|| git(repo, &["rev-parse", "--verify", "--quiet", &format!("{value}^{{tree}}")])).flatten();
    resolved.map_or_else(|| format!("value {value}"), |tree| format!("tree {tree}"))
}

pub fn key(check: &Check) -> Option<String> {
    let mut paths = selectors(check.repo, check.tree, check.command)?;
    paths.extend(TOOLCHAIN.iter().map(|path| (*path).to_owned()));
    let (named, text) = named(check.repo, check.tree, check.command);
    paths.extend(named);
    if BUILDS_NORTH.iter().any(|marker| check.command.contains(marker)) {
        paths.extend(rust_build_input_paths().map(str::to_owned));
    }
    let specs = object_specs(check.tree, &paths);
    let resolved = objects(check.repo, &specs)?;
    if resolved.lines().count() != specs.len() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(format!("command\0{}\nruntime\0{}\n", check.command, check.runtime));
    for (spec, object) in specs.iter().zip(resolved.lines()) {
        let path = spec.split_once(':').map_or("", |(_, path)| path);
        let object = if object.contains(' ') { "missing" } else { object };
        hasher.update(format!("path\0{path}\0{object}\n"));
    }
    for (name, value) in check.environment.iter().filter(|(name, _)| reads_variable(check.command, &text, name)) {
        hasher.update(format!("env\0{name}\0{}\n", environment_value(check.repo, value)));
    }
    if machine_bound(check.command) {
        for (name, repo) in check.machine_repos {
            hasher.update(format!("machine\0{name}\0{}\n", git(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap_or_default()));
        }
    }
    Some(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn runtime_identity() -> String {
    let Ok(path) = std::env::current_exe().and_then(std::fs::canonicalize) else { return String::new() };
    let stamp = std::fs::metadata(&path).ok().map(|metadata| {
        let modified = metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |since| since.as_nanos());
        format!("{} {modified}", metadata.len())
    });
    format!("{} {}", path.display(), stamp.unwrap_or_default())
}
