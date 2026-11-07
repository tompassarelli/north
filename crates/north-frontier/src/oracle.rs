use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use north_base::error::NorthError;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::refusal;
use super::sandbox::git_text;
use super::task::{Target, Task};

/// The landing's tests as the grader holds them: those passing at the landing (twice), those already
/// passing at the base with the hidden tests laid over it, and the difference that the change must make pass.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Oracle {
    pub reference: BTreeSet<String>,
    pub base: BTreeSet<String>,
    pub fail_to_pass: BTreeSet<String>,
}

/// One test run's verdicts by test id (`BINARY NAME`).
pub type Verdicts = BTreeMap<String, bool>;

fn status_line() -> Regex {
    Regex::new(r"^\s*(PASS|FAIL|FLAKY|TIMEOUT|SIGSEGV|SIGABRT|SIGKILL|ABORT|LEAK|LEAK-FAIL)\s+\[[^\]]*\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(\S+)").unwrap_or_else(|_| unreachable!())
}

pub fn parse(output: &str) -> Verdicts {
    let line = status_line();
    output.lines().filter_map(|text| line.captures(text)).map(|found| (format!("{} {}", &found[2], &found[3]), matches!(&found[1], "PASS" | "FLAKY" | "LEAK"))).collect()
}

fn expression(targets: &[Target]) -> String {
    targets.iter().map(|target| format!("(package({}) & {})", target.package, target.filter)).collect::<Vec<_>>().join(" | ")
}

/// Runs the task's test slice in a checkout inside its own dev shell, with no north on PATH, so the
/// repository's capacity-gate shims fall through to the real cargo.
pub fn run_tests(view: &[String], tree: &Path, task: &Task, log: &Path, target: Option<&Path>) -> Result<Verdicts, NorthError> {
    let packages: BTreeSet<&str> = task.targets.iter().map(|target| target.package.as_str()).collect();
    let mut argv = super::process::leased(&format!("frontier-grade-{}", task.id), std::time::Duration::from_secs(3600))?;
    argv.extend(view.iter().cloned());
    argv.extend(["env".into(), "-C".into(), tree.display().to_string(), "nix".into()]);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.args(["develop", "--command", "cargo", "nextest", "run", "--no-fail-fast", "--color", "never", "--status-level", "all", "--final-status-level", "none"]);
    for package in packages {
        command.args(["-p", package]);
    }
    command.args(["-E", &expression(&task.targets)]).env("PATH", super::sandbox::host_path()).env_remove("CARGO_TARGET_DIR");
    if let Some(target) = target {
        command.env("CARGO_TARGET_DIR", target);
    }
    let output = command.output().map_err(NorthError::Io)?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    fs::write(log, &text).map_err(NorthError::Io)?;
    Ok(parse(&text))
}

fn passing(verdicts: &Verdicts) -> BTreeSet<String> {
    verdicts.iter().filter(|(_, passed)| **passed).map(|(id, _)| id.clone()).collect()
}

/// Writes the landing's version of each hidden test file into the tree, and the module line that
/// declares a new one where the tree's parent module lacks it.
pub fn overlay(tree: &Path, source: &Path, task: &Task) -> Result<(), NorthError> {
    for path in &task.hidden {
        let target = tree.join(path);
        match git_text(source, &["show", &format!("{}:{path}", task.landed)]) {
            Ok(text) => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(NorthError::Io)?;
                }
                fs::write(&target, text + "\n").map_err(NorthError::Io)?;
                declare(tree, source, task, path)?;
            }
            Err(_) => {
                let _ = fs::remove_file(&target);
            }
        }
    }
    Ok(())
}

fn declare(tree: &Path, source: &Path, task: &Task, path: &str) -> Result<(), NorthError> {
    let file = Path::new(path);
    let (Some(dir), Some(name), Some(stem)) = (file.parent(), file.file_name().and_then(|name| name.to_str()), file.file_stem().and_then(|stem| stem.to_str())) else { return Ok(()) };
    let listing = git_text(source, &["ls-tree", "--name-only", &format!("{}:{}", task.landed, dir.display())]).unwrap_or_default();
    let mut parents: Vec<String> = listing.lines().filter(|entry| entry.ends_with(".rs") && *entry != name).map(|entry| dir.join(entry).display().to_string()).collect();
    if let (Some(up), Some(dir_name)) = (dir.parent(), dir.file_name()) {
        parents.push(up.join(format!("{}.rs", dir_name.to_string_lossy())).display().to_string());
    }
    let declares = Regex::new(r#"(?m)^(?:\s*#\[[^\n]*\]\s*\n)*\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;"#).map_err(|error| refusal(error.to_string()))?;
    for parent in parents {
        let Ok(landed) = git_text(source, &["show", &format!("{}:{parent}", task.landed)]) else { continue };
        let Some(line) = declares.find_iter(&landed).map(|found| found.as_str()).find(|found| found.contains(&format!("\"{name}\"")) || found.contains(&format!("mod {stem};"))) else { continue };
        let current = fs::read_to_string(tree.join(&parent)).unwrap_or_default();
        if !current.contains(line.trim()) {
            fs::write(tree.join(&parent), format!("{current}\n{}\n", line.trim())).map_err(NorthError::Io)?;
        }
        return Ok(());
    }
    Ok(())
}

fn checkout(source: &Path, at: &str, dir: &Path) -> Result<(), NorthError> {
    if dir.exists() {
        return Ok(());
    }
    git_text(source, &["worktree", "add", "--quiet", "--detach", &dir.display().to_string(), at]).map(|_| ())
}

/// Builds the oracle once per task: the landing's slice twice (a test must pass both times to count)
/// and the base with the hidden tests laid over it.
pub fn build(task: &Task, source: &Path, dir: &Path, target: &Path) -> Result<Oracle, NorthError> {
    let saved = dir.join("oracle.json");
    if let Ok(text) = fs::read_to_string(&saved) {
        return serde_json::from_str(&text).map_err(NorthError::Json);
    }
    fs::create_dir_all(dir).map_err(NorthError::Io)?;
    let landed = dir.join("landed");
    checkout(source, &task.landed, &landed)?;
    let first = passing(&run_tests(&[], &landed, task, &dir.join("landed-1.log"), Some(target))?);
    let second = passing(&run_tests(&[], &landed, task, &dir.join("landed-2.log"), Some(target))?);
    let reference: BTreeSet<String> = first.intersection(&second).cloned().collect();
    let base_tree = dir.join("base");
    checkout(source, &task.base, &base_tree)?;
    overlay(&base_tree, source, task)?;
    let base = passing(&run_tests(&[], &base_tree, task, &dir.join("base.log"), Some(target))?);
    if fs::read_to_string(dir.join("base.log")).is_ok_and(|log| log.contains("could not compile")) {
        return Err(refusal("the landing's tests do not build at the base: they use names the landing added, so a correct change under other names would fail them"));
    }
    let fail_to_pass = reference.difference(&base).cloned().collect();
    let oracle = Oracle { reference, base, fail_to_pass };
    fs::write(&saved, serde_json::to_string_pretty(&oracle).map_err(NorthError::Json)?).map_err(NorthError::Io)?;
    for tree in [&landed, &base_tree] {
        let _ = git_text(source, &["worktree", "remove", "--force", &tree.display().to_string()]);
    }
    Ok(oracle)
}
