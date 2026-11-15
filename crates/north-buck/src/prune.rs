use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

use crate::host_class::{self, LIST};

const HOST_CASES: &str = "[[package.metadata.north.buck.host-cases]]";

/// Drops each line of the host-case list and each manifest host-case entry whose test function the tree at `root` no longer defines, nothing in a tree without the list, reading the packages from the workspace manifest rather than Cargo, so a revert that removes a test takes every build entry naming it in the same commit; returns the files it rewrote.
pub fn prune(root: &Path) -> Result<Vec<PathBuf>, String> {
    let list = root.join(LIST);
    if !list.exists() {
        return Ok(Vec::new());
    }
    let packages = packages(root)?;
    let mut rewritten = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&list) {
        let kept: Vec<&str> = text.lines().filter(|line| listed_case_lives(root, &packages, line)).collect();
        if kept.len() != text.lines().count() {
            std::fs::write(&list, format!("{}\n", kept.join("\n"))).map_err(|why| format!("{LIST}: {why}"))?;
            rewritten.push(list);
        }
    }
    for (_, dir) in &packages {
        let manifest = root.join(dir).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).map_err(|why| format!("{}: {why}", manifest.display()))?;
        if let Some(pruned) = without_dead_cases(&root.join(dir), &text)? {
            std::fs::write(&manifest, pruned).map_err(|why| format!("{}: {why}", manifest.display()))?;
            rewritten.push(manifest);
        }
    }
    Ok(rewritten)
}

fn packages(root: &Path) -> Result<Vec<(String, String)>, String> {
    let workspace = read(&root.join("Cargo.toml"))?;
    let members = workspace.get("workspace").and_then(|table| table.get("members")).and_then(Item::as_array);
    let dirs = std::iter::once(".".to_owned()).chain(members.into_iter().flat_map(|members| members.iter().filter_map(|member| member.as_str().map(str::to_owned))));
    let mut packages = Vec::new();
    for dir in dirs {
        let manifest = read(&root.join(&dir).join("Cargo.toml"))?;
        if let Some(name) = manifest.get("package").and_then(|package| package.get("name")).and_then(Item::as_str) {
            packages.push((name.to_owned(), dir));
        }
    }
    Ok(packages)
}

fn read(manifest: &Path) -> Result<DocumentMut, String> {
    let text = std::fs::read_to_string(manifest).map_err(|why| format!("{}: {why}", manifest.display()))?;
    text.parse().map_err(|why| format!("{}: {why}", manifest.display()))
}

fn function(case: &str) -> &str {
    case.rsplit("::").next().unwrap_or(case)
}

fn listed_case_lives(root: &Path, packages: &[(String, String)], line: &str) -> bool {
    let entry = line.trim();
    let Some((binary, case)) = entry.split_once(' ').filter(|_| !entry.starts_with('#')) else { return true };
    let package = binary.split_once("::").map_or(binary, |(package, _)| package);
    packages.iter().find(|(name, _)| name == package).is_some_and(|(_, dir)| host_class::defines(&root.join(dir), function(case)))
}

fn without_dead_cases(dir: &Path, text: &str) -> Result<Option<String>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let headers: Vec<usize> = lines.iter().enumerate().filter(|(_, line)| line.trim_start().starts_with('[')).map(|(at, _)| at).collect();
    let mut dropped = vec![false; lines.len()];
    for (index, &start) in headers.iter().enumerate() {
        if lines[start].trim() != HOST_CASES {
            continue;
        }
        let end = headers.get(index + 1).copied().unwrap_or(lines.len());
        let body: DocumentMut = lines[start + 1..end].join("\n").parse().map_err(|why| format!("{}: a host case: {why}", dir.join("Cargo.toml").display()))?;
        let Some(name) = body.get("name").and_then(Item::as_str) else { continue };
        if host_class::defines(dir, function(name)) {
            continue;
        }
        let from = (0..start).rev().take_while(|&at| lines[at].trim().is_empty()).last().unwrap_or(start);
        let to = (start + 1..end).rev().find(|&at| !lines[at].trim().is_empty()).map_or(end, |at| at + 1);
        dropped[from..to].iter_mut().for_each(|line| *line = true);
    }
    if !dropped.contains(&true) {
        return Ok(None);
    }
    let kept: Vec<&str> = lines.iter().zip(&dropped).filter(|(_, dropped)| !**dropped).map(|(line, _)| *line).collect();
    Ok(Some(format!("{}\n", kept.join("\n"))))
}
