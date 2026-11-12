use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::proc::{git, must, sha256};

pub const SOURCE: &str = "https://github.com/pingdotgg/t3code";
pub const RELEASE: &str = "v0.1.9";
pub const PIN: &str = "24e77473ecf41bc1f7142529c7911c1c1aae1d76";
pub const CASES: &str = "evals/mcp-tool-use/cases";
pub const GRADERS: [&str; 5] = [
    "evals/mcp-tool-use/run.ts",
    "evals/mcp-tool-use/regrade.ts",
    "evals/mcp-tool-use/device-grading.ts",
    "evals/mcp-tool-use/file-link-grading.ts",
    "evals/mcp-tool-use/secret-grading.ts",
];

pub fn fetch(pin: &str, into: &Path) -> Result<(), String> {
    if !into.join(".git").exists() {
        fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
        must(git(into, &["init", "-q"]), "git init")?;
    }
    if at(into, pin).is_err() {
        must(git(into, &["fetch", "-q", "--depth", "1", SOURCE, pin]), "git fetch of the pinned commit")?;
        must(git(into, &["checkout", "-q", "--detach", "FETCH_HEAD"]), "git checkout of the pinned commit")?;
    }
    at(into, pin)
}

fn unchanged(checkout: &Path, pin: &str, under: &[&str]) -> Result<(), String> {
    let head = must(git(checkout, &["rev-parse", "HEAD"]), "git rev-parse HEAD")?;
    if head != pin {
        return Err(format!("{} is at {head}, not the pinned {pin}", checkout.display()));
    }
    let status = [&["status", "--porcelain", "--untracked-files=no", "--"][..], under].concat();
    let dirty = must(git(checkout, &status), "git status")?;
    if dirty.is_empty() { Ok(()) } else { Err(format!("{} has changed tracked files, so its cases and graders are not the pinned ones:\n{dirty}", checkout.display())) }
}

pub fn at(checkout: &Path, pin: &str) -> Result<(), String> {
    unchanged(checkout, pin, &["."])
}

pub fn graders_at(checkout: &Path, pin: &str) -> Result<(), String> {
    unchanged(checkout, pin, &["evals", "apps", "packages"])
}

pub fn hashes(checkout: &Path, paths: &[String]) -> Result<Value, String> {
    let mut files = Map::new();
    for path in paths {
        let blob = must(git(checkout, &["rev-parse", &format!("HEAD:{path}")]), "git rev-parse of a pinned file")?;
        files.insert(path.clone(), json!({"sha256": sha256(&checkout.join(path))?, "blob": blob}));
    }
    Ok(Value::Object(files))
}

pub fn case_files(checkout: &Path) -> Result<Vec<String>, String> {
    let listed = must(git(checkout, &["ls-tree", "-r", "--name-only", "HEAD", "--", CASES]), "git ls-tree of the case files")?;
    Ok(listed.lines().filter(|line| line.ends_with(".ndjson")).map(str::to_owned).collect())
}
