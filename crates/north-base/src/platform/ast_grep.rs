//! ast-grep scans of in-memory source text: each file is written under a scratch directory, every rule in
//! one YAML stream is run over it in one process, and each match comes back with its path relative to that
//! directory and its single metavariables.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub struct File<'a> {
    pub path: &'a str,
    pub text: &'a str,
}

#[derive(Clone, Debug)]
pub struct Match {
    pub rule: String,
    pub file: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub end_line: usize,
    pub text: String,
    pub single: BTreeMap<String, String>,
}

pub fn available() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("ast-grep").is_file()))
}

pub fn scan(files: &[File], rules: &str) -> Result<Vec<Match>, String> {
    if files.is_empty() || rules.trim().is_empty() {
        return Ok(Vec::new());
    }
    let scratch = tempfile::Builder::new().prefix("ast-grep-").tempdir().map_err(|error| format!("temp dir: {error}"))?;
    let tree = scratch.path().join("tree");
    for file in files {
        let out = tree.join(file.path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&out, file.text).map_err(|error| format!("{}: {error}", out.display()))?;
    }
    let rules_file = scratch.path().join("rules.yml");
    std::fs::write(&rules_file, rules).map_err(|error| format!("ast-grep rules: {error}"))?;
    run(&tree, &rules_file)
}

fn run(tree: &Path, rules_file: &Path) -> Result<Vec<Match>, String> {
    let mut command = Command::new("ast-grep");
    command.args(["scan", "--threads", "1", "-r"]).arg(rules_file).arg("--json=stream");
    for kind in ["hidden", "vcs", "dot", "parent", "global", "exclude"] {
        command.args(["--no-ignore", kind]);
    }
    let output = command.arg(tree).output().map_err(|error| format!("ast-grep: {error}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!("ast-grep: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let prefix = format!("{}/", tree.display());
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.is_empty()) {
        let found: serde_json::Value = serde_json::from_str(line).map_err(|error| format!("ast-grep output: {error}"))?;
        let number = |pointer: &str| found.pointer(pointer).and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
        let file = found["file"].as_str().unwrap_or_default();
        let single = found.pointer("/metaVariables/single").and_then(serde_json::Value::as_object).map(|single| single.iter().filter_map(|(name, value)| Some((name.clone(), value["text"].as_str()?.to_owned()))).collect()).unwrap_or_default();
        out.push(Match {
            rule: found["ruleId"].as_str().unwrap_or_default().to_owned(),
            file: file.strip_prefix(&prefix).unwrap_or(file).to_owned(),
            start: number("/range/byteOffset/start"),
            end: number("/range/byteOffset/end"),
            line: number("/range/start/line") + 1,
            end_line: number("/range/end/line") + 1,
            text: found["text"].as_str().unwrap_or_default().to_owned(),
            single,
        });
    }
    Ok(out)
}
