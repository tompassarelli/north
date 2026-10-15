use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::exec;
use super::notes::Notes;

const BUILD_WAIT: Duration = Duration::from_mins(25);
const QUERY_WAIT: Duration = Duration::from_secs(30);
const LOG_TAIL: usize = 20;

pub fn checkout(directory: &Path) -> Result<PathBuf, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(["rev-parse", "--show-toplevel"]);
    let root = PathBuf::from(exec::stdout(command, QUERY_WAIT, None)?.trim());
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    if manifest.lines().any(|line| line.trim() == "name = \"north\"") {
        Ok(root)
    } else {
        Err(format!("{} is not a north checkout; run inside one or pass --north PATH", root.display()))
    }
}

pub fn head(directory: &Path) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(["rev-parse", "HEAD"]);
    Ok(exec::stdout(command, QUERY_WAIT, None)?.trim().to_owned())
}

pub fn installed(program: &str) -> Result<PathBuf, String> {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return fs::canonicalize(path).map_err(|error| format!("{program}: {error}"));
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("{program} is not on PATH"))
}

pub fn build(root: &Path, notes: &Notes) -> Result<PathBuf, String> {
    let helper = capacity_helper()?;
    let mut command = Command::new("bun");
    command
        .arg(helper)
        .args(["run", "--owner", "verify-north", "--timeout-seconds", &BUILD_WAIT.as_secs().to_string(), "--"])
        .args(["nix", "develop", "-c", "cargo", "build", "--offline", "--bin", "north"])
        .current_dir(root);
    let output = exec::output(command, BUILD_WAIT + QUERY_WAIT, Some(notes))?;
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let binary = target_directory(root).join("debug/north");
    if !output.status.success() || !log.contains("Finished") || !binary.is_file() {
        let lines: Vec<&str> = log.lines().collect();
        let tail = lines[lines.len().saturating_sub(LOG_TAIL)..].join("\n");
        return Err(format!("cargo build exited {} without a fresh {}; output ends:\n{tail}", output.status.code().unwrap_or(-1), binary.display()));
    }
    Ok(binary)
}

fn capacity_helper() -> Result<PathBuf, String> {
    let mut command = Command::new("agents");
    command.args(["path", "machine-capacity"]);
    let skill = PathBuf::from(exec::stdout(command, QUERY_WAIT, None)?.trim());
    let helper = skill.parent().map(|directory| directory.join("scripts/machine-capacity.mjs"));
    helper.filter(|helper| helper.is_file()).ok_or_else(|| format!("no machine-capacity helper beside {}", skill.display()))
}

fn target_directory(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(directory) if directory.is_absolute() => directory,
        Some(directory) => root.join(directory),
        None => root.join("target"),
    }
}
