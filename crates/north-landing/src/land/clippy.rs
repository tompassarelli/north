use std::path::Path;
use std::process::{Command, Stdio};

use super::enqueue::git;
use super::refusal;

const ARGUMENTS: [&str; 7] = ["clippy", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"];

fn changes_rust(root: &Path, base: &str, head: &str) -> Result<bool, String> {
    let range = format!("{base}...{head}");
    Ok(!git(root, &["diff", "--name-only", &range, "--", "*.rs"])?.is_empty())
}

pub fn check(root: &Path, base: &str, head: &str) -> Result<(), String> {
    if !root.join("Cargo.toml").is_file() || !changes_rust(root, base, head)? {
        return Ok(());
    }
    println!("safe-push: running cargo {} before queueing the lane", ARGUMENTS.join(" "));
    let output = Command::new("cargo")
        .current_dir(root)
        .args(ARGUMENTS)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("clippy could not run cargo: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let text = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    Err(format!("clippy refused the lane (cargo {}):\n{}", ARGUMENTS.join(" "), refusal::errors(&text)))
}
