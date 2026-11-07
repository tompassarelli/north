//! The purity gate: a strategy crate cannot reach the clock, files, network or environment, because it
//! is `#![no_std]`, takes no dependency outside its allowlist and has no build script, `std` path,
//! `include_*!`, `env!` or state held between calls (docs/rationale/strategy-kernel.md, Crates, calls and versions).
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use toml_edit::DocumentMut;

use super::git;

/// Each strategy crate and the only dependencies it may take.
const CRATES: [(&str, &[&str]); 2] = [("crates/north-strategy-types", &["serde"]), ("crates/north-strategy", &["north-strategy-types", "libm"])];

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

static FORBIDDEN: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"\bstd\s*::", "reaches std, which has the clock, files, network and environment"),
        (r"\bextern\s+crate\s+std\b", "links std back in"),
        (r"\binclude(_str|_bytes)?\s*!", "reads a file of the host at build time"),
        (r"\b(option_)?env\s*!", "reads the build environment"),
        (r"\bstatic\s+mut\b", "holds state between calls"),
        (r"\bthread_local\s*!", "holds state between calls"),
        (r"\bAtomic[A-Z]\w*", "holds state between calls"),
        (r"\bcore\s*::\s*cell\b", "holds state between calls"),
        (r"\bunsafe\b", "steps outside the language's checks"),
    ]
    .into_iter()
    .map(|(pattern, why)| (Regex::new(pattern).expect("the pattern is valid"), why))
    .collect()
});

fn code(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _)| code)
}

/// What is wrong with the strategy crate's manifest.
fn manifest_findings(manifest: &str, allowed: &[&str]) -> Vec<String> {
    let Ok(document) = manifest.parse::<DocumentMut>() else {
        return vec!["Cargo.toml does not parse".to_owned()];
    };
    let mut found = Vec::new();
    if document.get("package").and_then(|package| package.get("build")).is_some() {
        found.push("Cargo.toml names a build script".to_owned());
    }
    if document.get("target").is_some() {
        found.push("Cargo.toml has target-specific dependencies, which the allowlist cannot judge".to_owned());
    }
    for table in DEPENDENCY_TABLES {
        let Some(dependencies) = document.get(table).and_then(|item| item.as_table_like()) else { continue };
        for (name, _) in dependencies.iter().filter(|(name, _)| !allowed.contains(name)) {
            found.push(format!("Cargo.toml [{table}] takes {name}, outside the allowlist ({})", allowed.join(", ")));
        }
    }
    found
}

/// What is wrong with one source file; `root` marks the crate's lib.rs, which must seal the crate.
fn source_findings(path: &str, source: &str, root: bool) -> Vec<String> {
    let mut found = Vec::new();
    if root {
        for attribute in ["#![no_std]", "#![forbid(unsafe_code)]"] {
            if !source.lines().any(|line| code(line).trim() == attribute) {
                found.push(format!("{path} lacks {attribute}"));
            }
        }
    }
    for (number, line) in source.lines().enumerate() {
        let line = code(line);
        for (pattern, why) in FORBIDDEN.iter() {
            if let Some(hit) = pattern.find(line) {
                found.push(format!("{path}:{} `{}` {why}", number + 1, hit.as_str().trim()));
            }
        }
    }
    found
}

fn tree(directory: &Path, head: &str, crate_path: &str) -> Vec<String> {
    git::text(directory, &["ls-tree", "-r", "--name-only", head, "--", crate_path]).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// Every finding for the strategy crates present at `head`.
fn findings(directory: &Path, head: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (crate_path, allowed) in CRATES {
        let files = tree(directory, head, crate_path);
        if files.is_empty() {
            continue;
        }
        let read = |path: &str| git::text(directory, &["show", &format!("{head}:{path}")]).unwrap_or_default();
        found.extend(manifest_findings(&read(&format!("{crate_path}/Cargo.toml")), allowed).into_iter().map(|finding| format!("{crate_path}: {finding}")));
        for path in &files {
            if path == &format!("{crate_path}/build.rs") {
                found.push(format!("{path} is a build script"));
            } else if path.starts_with(&format!("{crate_path}/src/")) && path.ends_with(".rs") {
                found.extend(source_findings(path, &read(path), path == &format!("{crate_path}/src/lib.rs")));
            }
        }
    }
    found
}

/// Refuses a head whose strategy crates can reach anything but their case; returns 0 when they cannot.
pub fn scan(directory: &Path, head: &str, out: &mut impl Write) -> u8 {
    let found = findings(directory, head);
    for finding in &found {
        let _ = writeln!(out, "strategy-purity: {finding}");
    }
    if !found.is_empty() {
        let _ = writeln!(out, "strategy-purity: a strategy is a pure function of its recorded case; give the kernel the input and keep the crate no_std (docs/rationale/strategy-kernel.md)");
    }
    u8::from(!found.is_empty())
}

#[cfg(test)]
pub(super) mod tests;
