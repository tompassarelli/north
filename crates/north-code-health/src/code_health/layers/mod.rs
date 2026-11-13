mod manifest_edges;
mod references;
mod rules;

use super::rust_modules::Crate;
use super::source::Source;
pub(super) use manifest_edges::{ManifestEdge, manifest_edges};
use references::references;
use rules::{Breach, Rule, Rules};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const USAGE: &str = "usage: north code-health layers [--repo DIR] BASE HEAD";
const PLAN: &str = "docs/rationale/crate-split.md#next-cuts";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub module: String,
    pub may_use: Vec<String>,
}

pub fn placement(structure: &str, path: &str) -> Option<Placement> {
    let rules = Rules::parse(structure).ok()?;
    rules.module_of(path).map(|(module, may_use)| Placement { module: module.to_owned(), may_use: may_use.to_vec() })
}

pub(super) fn run(arguments: &[String]) -> u8 {
    let (repo, range) = match arguments {
        [flag, dir, rest @ ..] if flag == "--repo" => (PathBuf::from(dir), rest),
        rest => (PathBuf::from("."), rest),
    };
    let [base, head] = range else {
        eprintln!("{USAGE}");
        return 2;
    };
    match verdict(&repo, base, head) {
        Ok((lines, code)) => {
            for line in lines {
                println!("{line}");
            }
            code
        }
        Err(error) => {
            eprintln!("north code-health layers: {error}");
            2
        }
    }
}

struct Revision<'a> {
    repo: &'a Path,
    rev: &'a str,
    source: Source,
    crates: Vec<Crate>,
}

impl<'a> Revision<'a> {
    fn at(repo: &'a Path, rev: &'a str) -> Result<Self, String> {
        let paths: Vec<String> = git(repo, &["ls-tree", "-r", "-z", "--name-only", "--full-tree", rev])?.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
        let manifests: Vec<(String, String)> = paths
            .iter()
            .filter(|path| *path == "Cargo.toml" || path.ends_with("/Cargo.toml"))
            .map(|path| show(repo, rev, path).map(|text| (path.clone(), text.unwrap_or_default())))
            .collect::<Result<_, _>>()?;
        let crates = Crate::from_manifests(manifests.iter().map(|(path, text)| (path.as_str(), text.as_str())));
        Ok(Self { repo, rev, source: Source::tree(paths), crates })
    }

    fn text(&self, path: &str) -> Result<Option<String>, String> {
        show(self.repo, self.rev, path)
    }

    fn breaches(&self, rules: &Rules, path: &str) -> Result<Vec<(usize, String, Breach)>, String> {
        let Some(text) = self.text(path)? else { return Ok(Vec::new()) };
        Ok(references(path, &text, &self.source, &self.crates)
            .into_iter()
            .flat_map(|reference| rules.breaches(path, &reference.target).into_iter().map(move |breach| (reference.line, reference.target.clone(), breach)))
            .collect())
    }
}

fn verdict(repo: &Path, base: &str, head: &str) -> Result<(Vec<String>, u8), String> {
    let Some(structure) = super::config::structure_at(repo, head)? else {
        return Ok((vec!["layers: no structure.toml at HEAD; nothing to check".to_owned()], 0));
    };
    let head_rules = Rules::parse(&structure)?;
    let base_rules = match super::config::structure_at(repo, base)? {
        Some(text) => Rules::parse(&text).unwrap_or_default().or(&head_rules),
        None => head_rules.clone(),
    };
    let (before, after) = (Revision::at(repo, base)?, Revision::at(repo, head)?);
    let changed = changes(repo, base, head)?;
    let mut lines = unplanned_crates(repo, base, head, &changed, &head_rules)?;
    for (old, new) in changed.into_iter().filter(|(_, new)| head_rules.checked(new)) {
        let known: BTreeSet<(Rule, String, String)> = match old {
            Some(old) => before.breaches(&base_rules, &old)?.into_iter().map(|(_, _, breach)| (breach.rule, breach.from, breach.to)).collect(),
            None => BTreeSet::new(),
        };
        let mut reported = BTreeSet::new();
        for (line, target, breach) in after.breaches(&head_rules, &new)? {
            if !known.contains(&(breach.rule, breach.from.clone(), breach.to.clone())) && reported.insert((breach.rule, breach.to.clone())) {
                lines.push(refusal(&new, line, &target, &breach));
            }
        }
    }
    let code = u8::from(!lines.is_empty());
    let summary = if lines.is_empty() {
        "layers: no new reference breaks structure.toml's module layers or crate DAG".to_owned()
    } else {
        format!("✗ layers: {} new reference(s) or crate(s) break structure.toml's module layers or crate DAG", lines.len())
    };
    lines.push(summary);
    Ok((lines, code))
}

fn unplanned_crates(repo: &Path, base: &str, head: &str, changed: &[(Option<String>, String)], rules: &Rules) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for (_, path) in changed {
        let Some(name) = path.strip_prefix("crates/").and_then(|rest| rest.strip_suffix("/Cargo.toml")).filter(|name| !name.contains('/')) else { continue };
        if show(repo, base, path)?.is_some() || crate_files(repo, head, name)?.iter().any(|file| rules.plans_crate(file)) {
            continue;
        }
        lines.push(format!("✗ crate-dag {path}: adds crate {name} with no [[crate_dag.crate]] entry in structure.toml, so no reference into or out of it is checked (plan {PLAN}); add its paths and may_use in this change"));
    }
    Ok(lines)
}

fn crate_files(repo: &Path, head: &str, name: &str) -> Result<Vec<String>, String> {
    let listing = git(repo, &["ls-tree", "-r", "--name-only", head, &format!("crates/{name}/src/")])?;
    Ok(listing.lines().map(str::to_owned).collect())
}

fn refusal(path: &str, line: usize, target: &str, breach: &Breach) -> String {
    let allowed = if breach.allowed.is_empty() { "nothing".to_owned() } else { breach.allowed.join(", ") };
    match breach.rule {
        Rule::Layer => format!(
            "✗ layer {path}:{line} new reference -> {target}: module {} may not use {} (its may_use: {allowed}); move the reference behind an allowed module's interface, or change [[module]] may_use in structure.toml in this change",
            breach.from, breach.to
        ),
        Rule::CrateDag => format!(
            "✗ crate-dag {path}:{line} new reference -> {target}: planned crate {} may not use {} (its may_use: {allowed}; plan {PLAN}); invert the dependency through a trait the lower crate owns, or move the code, or change [[crate_dag.crate]] in structure.toml in this change",
            breach.from, breach.to
        ),
    }
}

fn changes(repo: &Path, base: &str, head: &str) -> Result<Vec<(Option<String>, String)>, String> {
    let listing = git(repo, &["diff", "-M", "--name-status", "-z", base, head])?;
    let mut parts = listing.split('\0').filter(|part| !part.is_empty());
    let mut out = Vec::new();
    while let Some(status) = parts.next() {
        let first = parts.next().ok_or("git diff: truncated name-status")?.to_owned();
        match status.as_bytes()[0] {
            b'R' | b'C' => {
                let second = parts.next().ok_or("git diff: truncated rename")?.to_owned();
                out.push((status.starts_with('R').then_some(first), second));
            }
            b'D' => {}
            b'A' => out.push((None, first)),
            _ => out.push((Some(first.clone()), first)),
        }
    }
    Ok(out)
}

fn show(repo: &Path, rev: &str, path: &str) -> Result<Option<String>, String> {
    let object = format!("{rev}:{path}");
    let exists = Command::new("git").arg("-C").arg(repo).args(["cat-file", "-e", &object]).stdin(Stdio::null()).stderr(Stdio::null()).status().map_err(|error| format!("git: {error}"))?;
    if !exists.success() {
        return Ok(None);
    }
    git(repo, &["show", &object]).map(Some)
}

fn git(repo: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(repo).args(arguments).stdin(Stdio::null()).output().map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests;
