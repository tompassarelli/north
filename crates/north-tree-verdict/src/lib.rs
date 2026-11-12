//! One verdict per tree (north#902, live check): the verdict-bearing form of a `cargo clippy` run and its cache key, which the capacity gate records when a fleet session's own clippy passes on a clean tree and the land checks look up before running the same clippy on that tree. It depends on std, git and sha2 alone.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

const VERDICT_NEUTRAL: [&str; 5] = ["--locked", "--offline", "--frozen", "-q", "--quiet"];
const BUILD_ENVIRONMENT: [&str; 8] = ["RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC_WORKSPACE_WRAPPER", "CARGO_ENCODED_RUST", "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_UNSTABLE_"];
const TARGET_DIRECTORY: &str = "CARGO_TARGET_DIR";
const NOT_BUILD_CHANGING: [&str; 2] = [TARGET_DIRECTORY, "CARGO_BUILD_JOBS"];
const SHELL_SYNTAX: &str = "'\"`$|&;<>()\\*?[]{}";
const LAND_SHELL: [&str; 3] = ["./bin/land-shell", "--dev", "--"];

/// The verdict-bearing form of a `cargo clippy` invocation: flags that cannot change its verdict dropped and the package set order-free, or None for anything that is not clippy.
pub fn canonical(arguments: &[String]) -> Option<String> {
    let split = arguments.iter().position(|word| word == "--").unwrap_or(arguments.len());
    let (cargo, lints) = arguments.split_at(split);
    let (subcommand, flags) = cargo.split_first()?;
    if subcommand != "clippy" {
        return None;
    }
    let flags = Flags::of(flags)?;
    let selection = match (flags.workspace, flags.packages.is_empty()) {
        (true, true) => "workspace".to_owned(),
        (false, false) => flags.packages.into_iter().collect::<Vec<_>>().join(","),
        _ => return None,
    };
    Some(format!("clippy\0{}\0{selection}\0{}", flags.others.join("\x1f"), lints.join("\x1f")))
}

#[derive(Default)]
struct Flags {
    packages: BTreeSet<String>,
    workspace: bool,
    others: Vec<String>,
}

impl Flags {
    fn of(flags: &[String]) -> Option<Self> {
        let mut parsed = Self::default();
        let mut words = flags.iter();
        while let Some(word) = words.next() {
            match word.as_str() {
                "--color" => {
                    words.next()?;
                }
                "-p" | "--package" => {
                    parsed.packages.insert(words.next()?.clone());
                }
                "--workspace" | "--all" => parsed.workspace = true,
                other => parsed.single(other),
            }
        }
        Some(parsed)
    }

    fn single(&mut self, word: &str) {
        if let Some(package) = word.strip_prefix("--package=") {
            self.packages.insert(package.to_owned());
        } else if !VERDICT_NEUTRAL.contains(&word) && !word.starts_with("--color=") {
            self.others.push(word.to_owned());
        }
    }
}

/// The cache key of a clippy run on `tree`, the same whether a fleet session's own run records it or a land check looks it up.
pub fn key(tree: &str, canonical: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("own-run\0{tree}\0{canonical}"));
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether an environment variable can change what cargo compiles or lints, so a run under it says nothing about a land check's. The capacity lease sets its own target directory and job count, neither of which changes a clippy verdict.
pub fn changes_build(name: &str) -> bool {
    !NOT_BUILD_CHANGING.contains(&name) && BUILD_ENVIRONMENT.iter().any(|prefix| name.starts_with(prefix))
}

/// The cargo arguments a `CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo ...` land line runs once its `${NAME:-default}` words are expanded, or None for any other shape.
pub fn land_arguments(command: &str, environment: &[(String, String)]) -> Option<Vec<String>> {
    let expanded = expand(command, environment)?;
    if expanded.contains(|c| SHELL_SYNTAX.contains(c)) {
        return None;
    }
    let words: Vec<&str> = expanded.split_whitespace().collect();
    let cargo = words.iter().position(|word| *word == "cargo")?;
    let (prefix, rest) = words.split_at(cargo);
    let wrapper = prefix.iter().skip_while(|word| word.starts_with(&format!("{TARGET_DIRECTORY}="))).copied().collect::<Vec<_>>();
    (wrapper == LAND_SHELL).then(|| rest[1..].iter().map(|word| (*word).to_owned()).collect())
}

fn expand(command: &str, environment: &[(String, String)]) -> Option<String> {
    let mut out = String::new();
    let mut rest = command;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let end = rest[start..].find('}')? + start;
        let inner = &rest[start + 2..end];
        let (name, default) = inner.split_once(":-").map_or((inner, None), |(name, default)| (name, Some(default)));
        let value = environment.iter().find(|(set, _)| set == name).map(|(_, value)| value.as_str()).filter(|value| !value.is_empty());
        out.push_str(value.or(default)?);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The tree object `revision` names in `repo`.
pub fn tree(repo: &Path, revision: &str) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(repo).args(["rev-parse", "--verify", "--quiet", &format!("{revision}^{{tree}}")]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !tree.is_empty()).then_some(tree)
}

/// HEAD's tree when `directory` is its worktree's top level and holds no change or untracked file git sees, so what ran there is exactly that tree.
pub fn clean_tree(directory: &Path) -> Option<String> {
    let git = |arguments: &[&str]| Command::new("git").arg("-C").arg(directory).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let top = git(&["rev-parse", "--show-toplevel"])?;
    if std::fs::canonicalize(top.trim()).ok()? != std::fs::canonicalize(directory).ok()? {
        return None;
    }
    git(&["status", "--porcelain", "--untracked-files=normal"])?.trim().is_empty().then(|| tree(directory, "HEAD")).flatten()
}

#[cfg(test)]
mod tests;
