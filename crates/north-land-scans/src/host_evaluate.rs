use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

use crate::{Range, git};

const SELF: &str = "host-evaluate";
const FLAKE_FILES: [&str; 2] = ["flake.nix", "flake.lock"];
const NIX_FEATURES: &str = "nix-command flakes";

static PATH_LITERAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\.?/[A-Za-z0-9_./+-]*").expect("nix path literal pattern"));

pub trait Evaluate: Sync {
    fn hosts(&self, directory: &Path) -> Result<Vec<String>, String>;
    fn toplevel(&self, directory: &Path, host: &str) -> Result<(), String>;
}

pub struct Nix;

impl Nix {
    fn eval(directory: &Path, attribute: &str, extra: &[&str]) -> Result<String, String> {
        let output = Command::new("nix")
            .current_dir(directory)
            .args(["--extra-experimental-features", NIX_FEATURES, "eval"])
            .args(extra)
            .arg(attribute)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("nix did not start: {error}"))?;
        match output.status.success() {
            true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
            false => Err(String::from_utf8_lossy(&output.stderr).lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")),
        }
    }
}

impl Evaluate for Nix {
    fn hosts(&self, directory: &Path) -> Result<Vec<String>, String> {
        let listed = Self::eval(directory, ".#nixosConfigurations", &["--json", "--apply", "builtins.attrNames"])?;
        serde_json::from_str(&listed).map_err(|error| format!("nix listed no host names: {error}"))
    }

    fn toplevel(&self, directory: &Path, host: &str) -> Result<(), String> {
        Self::eval(directory, &format!(".#nixosConfigurations.\"{host}\".config.system.build.toplevel.drvPath"), &["--raw"]).map(drop)
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut parts: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if parts.last().is_some_and(|last| matches!(last, Component::Normal(_))) => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.into_iter().collect()
}

fn nix_files_under(directory: &Path, relative: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory.join(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let path = relative.join(entry.file_name());
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => nix_files_under(directory, &path, found),
            Ok(_) if path.extension().is_some_and(|extension| extension == "nix") => found.push(path),
            _ => {}
        }
    }
}

pub(crate) fn imported(directory: &Path, file: &Path, text: &str) -> Vec<PathBuf> {
    let parent = file.parent().unwrap_or(Path::new(""));
    PATH_LITERAL
        .find_iter(text)
        .map(|literal| normalize(&parent.join(literal.as_str())))
        .map(|target| if directory.join(&target).is_dir() { target.join("default.nix") } else { target })
        .collect()
}

fn closure(directory: &Path, host: &str) -> BTreeSet<PathBuf> {
    let mut pending = vec![PathBuf::from(format!("hosts/{host}.nix"))];
    nix_files_under(directory, &PathBuf::from(format!("hosts/{host}")), &mut pending);
    let mut seen = BTreeSet::new();
    while let Some(file) = pending.pop() {
        if !directory.join(&file).is_file() || !seen.insert(file.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(directory.join(&file)).unwrap_or_default();
        pending.extend(imported(directory, &file, &text));
    }
    seen
}

pub(crate) fn changed_nix(directory: &Path, fork: &str, head: &str) -> Vec<String> {
    git::text(directory, &["diff", "--name-only", "--no-renames", fork, head])
        .unwrap_or_default()
        .lines()
        .filter(|path| path.ends_with(".nix") || *path == "flake.lock")
        .map(str::to_owned)
        .collect()
}

pub fn affected(directory: &Path, hosts: Vec<String>, changed: &[String]) -> Vec<String> {
    if changed.iter().any(|path| FLAKE_FILES.contains(&path.as_str())) {
        return hosts;
    }
    hosts.into_iter().filter(|host| closure(directory, host).iter().any(|file| changed.iter().any(|path| Path::new(path) == file))).collect()
}

pub fn check(directory: &Path, base: &str, head: &str, nix: &impl Evaluate) -> Result<(), String> {
    let changed = changed_nix(directory, &git::merge_base(directory, base, head), head);
    if changed.is_empty() || !directory.join("flake.nix").is_file() {
        return Ok(());
    }
    let hosts = affected(directory, nix.hosts(directory)?, &changed);
    let results: Vec<(String, Result<(), String>)> = std::thread::scope(|scope| {
        let running: Vec<_> = hosts.into_iter().map(|host| scope.spawn(move || (host.clone(), nix.toplevel(directory, &host)))).collect();
        running.into_iter().filter_map(|job| job.join().ok()).collect()
    });
    let refusals: Vec<String> = results
        .into_iter()
        .filter_map(|(host, result)| result.err().map(|error| format!("{host} cannot evaluate its toplevel:\n{}", error.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))))
        .collect();
    match refusals.is_empty() {
        true => Ok(()),
        false => Err(refusals.join("\n")),
    }
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "{SELF}: expected BASE..HEAD, got {range}");
        return 2;
    };
    match check(directory, base, head, &Nix) {
        Ok(()) => 0,
        Err(refusal) => {
            let _ = writeln!(out, "{SELF}: refused: a host configuration the lane changes cannot be evaluated:");
            for line in refusal.lines() {
                let _ = writeln!(out, "  {line}");
            }
            1
        }
    }
}
