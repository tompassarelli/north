use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

use super::{ancestor, changed, git_lines};

pub struct Seen {
    pub sha: String,
    pub failed: BTreeSet<String>,
    pub names_its_failures: bool,
}

pub fn before<'a>(repo: &Path, earlier: &'a [Seen], sha: &str) -> Option<&'a Seen> {
    earlier.iter().rev().find(|seen| seen.sha != sha && seen.names_its_failures && ancestor(repo, &seen.sha, sha))
}

/// A test is flaky when it failed on a fresh commit, a later full run passed it, and nothing between the run before and that passing run touched the crate holding it or a crate that crate depends on.
pub fn reaches_nothing(repo: &Path, seen: &[Seen], at: usize, test: &str) -> bool {
    let failing = &seen[at];
    let Some(base) = before(repo, &seen[..at], &failing.sha) else { return false };
    let passing = seen[at + 1..].iter().find(|later| later.sha != failing.sha && later.names_its_failures && !later.failed.contains(test) && ancestor(repo, &failing.sha, &later.sha));
    let Some(passing) = passing else { return false };
    let Some(inputs) = inputs(repo, &passing.sha, test) else { return false };
    !changed(repo, &base.sha, &passing.sha).iter().any(|file| inputs.iter().any(|input| file == input || input.ends_with('/') && file.starts_with(input.as_str())))
}

fn inputs(repo: &Path, sha: &str, test: &str) -> Option<BTreeSet<String>> {
    let leaf = test.rsplit("::").next()?;
    let found = git_lines(repo, &["grep", "-l", "-F", "-w", "-e", &format!("fn {leaf}"), sha, "--", "*.rs"]);
    let mut crates: Vec<String> = Vec::new();
    for line in &found {
        let path = line.strip_prefix(&format!("{sha}:"))?;
        let directory = Path::new(path).ancestors().skip(1).map(|ancestor| ancestor.to_string_lossy().into_owned()).find(|directory| has_manifest(repo, sha, directory))?;
        if !crates.contains(&directory) {
            crates.push(directory);
        }
    }
    let mut next = 0;
    while let Some(directory) = crates.get(next).cloned() {
        next += 1;
        for dependency in path_dependencies(repo, sha, &directory) {
            if !crates.contains(&dependency) {
                crates.push(dependency);
            }
        }
    }
    (!crates.is_empty()).then(|| crates.iter().flat_map(|directory| if directory.is_empty() { ["src/", "tests/", "build.rs", "Cargo.toml"].map(str::to_owned).to_vec() } else { vec![format!("{directory}/")] }).collect())
}

fn manifest(directory: &str) -> String {
    if directory.is_empty() { "Cargo.toml".to_owned() } else { format!("{directory}/Cargo.toml") }
}

fn has_manifest(repo: &Path, sha: &str, directory: &str) -> bool {
    Command::new("git").arg("-C").arg(repo).args(["cat-file", "-e", &format!("{sha}:{}", manifest(directory))]).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn path_dependencies(repo: &Path, sha: &str, directory: &str) -> Vec<String> {
    git_lines(repo, &["show", &format!("{sha}:{}", manifest(directory))])
        .iter()
        .filter_map(|line| line.split_once("path = \"").and_then(|(_, rest)| rest.split_once('"')).map(|(path, _)| join(directory, path)))
        .collect()
}

fn join(directory: &str, relative: &str) -> String {
    let mut parts: Vec<String> = directory.split('/').filter(|part| !part.is_empty()).map(str::to_owned).collect();
    for component in Path::new(relative).components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}
