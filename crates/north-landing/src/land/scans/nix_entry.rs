use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::git;

const USAGE: &str = "usage: north land scan nix-entry [REVISION]";
const SCRIPT_DIRS: [&str; 3] = ["harness/scripts/", "bin/", "harness/lib/"];
const NESTED_DIRS: [&str; 2] = ["harness/scripts/", "harness/lib/"];
const SETTINGS: &str = ".safe-push";
const SHELL_ENTRY: &str = "bin/land-shell";
const START: &str = r#"(?:^|[;&|(=]|\$\(|\bexec\s|\s--\s)\s*(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*"?(?:[^\s"]*/)?"#;

static NIX_DEVELOP: LazyLock<Regex> = LazyLock::new(|| form(r"nix\s+develop\b"));
static NIX_SHELL: LazyLock<Regex> = LazyLock::new(|| form(r"nix-shell\b"));
static LAND_SHELL_DEV: LazyLock<Regex> = LazyLock::new(|| form(r#"land-shell"?\s+--dev\b"#));
static AD_HOC: LazyLock<Regex> = LazyLock::new(|| form(r"(?:nix\s+(?:shell|run)\s+(?:[^\s;&|]+\s+)*?nixpkgs#|nix-shell\s+(?:[^\s;&|]+\s+)*?-p\b)"));

fn form(pattern: &str) -> Regex {
    Regex::new(&format!("{START}{pattern}")).expect("nix entry pattern")
}

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub reason: &'static str,
}

pub fn has_dev_shell(flake: &str) -> bool {
    flake.contains("devShell")
}

fn command_text<'a>(path: &str, line: &'a str) -> Option<&'a str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    if path != SETTINGS {
        return Some(line);
    }
    let (_, value) = trimmed.split_once('=')?;
    Some(value.split_once(" :: ").map_or(value, |(_, command)| command))
}

fn reason(path: &str, command: &str, dev_shell: bool) -> Option<&'static str> {
    if dev_shell && AD_HOC.is_match(command) {
        return Some("an ad-hoc nixpkgs toolchain beside the flake's dev shell; add the package to flake.nix's dev shell and run through `north dev-shell run --`");
    }
    if path != SHELL_ENTRY && (NIX_DEVELOP.is_match(command) || NIX_SHELL.is_match(command)) {
        return Some("a literal Nix dev-shell entry, which needs nix on PATH and nests shells; run the command through `north dev-shell run --`");
    }
    if NESTED_DIRS.iter().any(|dir| path.starts_with(dir)) && LAND_SHELL_DEV.is_match(command) {
        return Some("`land-shell --dev` inside a script a land check already runs in the dev shell; run the tool directly or through `north dev-shell run --`");
    }
    None
}

pub fn in_scope(path: &str) -> bool {
    path == SETTINGS || SCRIPT_DIRS.iter().any(|dir| path.starts_with(dir))
}

pub fn findings(path: &str, text: &str, dev_shell: bool) -> Vec<Finding> {
    text.lines()
        .enumerate()
        .filter_map(|(at, line)| {
            let command = command_text(path, line)?;
            reason(path, command, dev_shell).map(|reason| Finding { path: path.to_owned(), line: at + 1, reason })
        })
        .collect()
}

pub fn run(directory: &Path, arguments: &[String]) -> u8 {
    let revision = match arguments {
        [] => "HEAD",
        [revision] if !revision.starts_with('-') => revision.as_str(),
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    let Some(listing) = git::text(directory, &["ls-tree", "-r", "--name-only", revision]) else {
        eprintln!("nix-entry: cannot list the tree of {revision}");
        return 1;
    };
    let show = |path: &str| git::bytes(directory, &["show", &format!("{revision}:{path}")]).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let dev_shell = show("flake.nix").is_some_and(|flake| has_dev_shell(&flake));
    let found: Vec<Finding> = listing.lines().filter(|path| in_scope(path)).flat_map(|path| findings(path, &show(path).unwrap_or_default(), dev_shell)).collect();
    for finding in &found {
        eprintln!("nix-entry: {}:{}: {}", finding.path, finding.line, finding.reason);
    }
    u8::from(!found.is_empty())
}

#[cfg(test)]
#[path = "nix_entry_tests.rs"]
mod tests;
