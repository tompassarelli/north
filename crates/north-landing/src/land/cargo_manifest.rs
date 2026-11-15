use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::platform::process;

const ARGUMENTS: [&str; 5] = ["metadata", "--no-deps", "--offline", "--format-version", "1"];
const CARGO_FAILED: i32 = 101;
const JAIL: [&str; 3] = ["--worker-sandbox", "--candidate", "--"];
const OUTSIDE_THE_TREE: &str = "/tmp";

pub enum Read {
    Metadata(String),
    Unreadable { line: String, log: String, named: Vec<String> },
    Unjudged { why: String, log: String },
}

pub fn read(tree: &Path, north: &Path, limit: Duration) -> Read {
    let manifest = tree.join("Cargo.toml");
    let line = format!("cargo {} --manifest-path {}", ARGUMENTS.join(" "), manifest.display());
    let mut command = Command::new(north);
    command.args(JAIL).args(["env", "-C", OUTSIDE_THE_TREE, "cargo"]).args(ARGUMENTS).arg("--manifest-path").arg(&manifest).current_dir(tree).stdin(Stdio::null());
    let output = match process::try_run_group_traced_within(command, limit) {
        Ok(Ok(output)) => output,
        Ok(Err(stalled)) => return Read::Unjudged { why: format!("`{line}` gave no answer within {} s: {stalled}", limit.as_secs()), log: String::new() },
        Err(error) => return Read::Unjudged { why: format!("`{line}`: {error}"), log: String::new() },
    };
    let log = String::from_utf8_lossy(&output.stderr).into_owned();
    match output.status.code() {
        Some(0) => Read::Metadata(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(CARGO_FAILED) => Read::Unreadable { named: named(&log, &[format!("{}/", tree.display()), relative(Path::new(OUTSIDE_THE_TREE), tree)]), line, log },
        code => Read::Unjudged { why: format!("`{line}` ended with {code:?} and named no manifest error"), log },
    }
}

fn named(log: &str, prefixes: &[String]) -> Vec<String> {
    log.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "Caused by:" && !line.starts_with(|first: char| first == '|' || first.is_ascii_digit()))
        .map(|line| prefixes.iter().fold(line.to_owned(), |line, prefix| line.replace(prefix, "")))
        .collect()
}

fn relative(from: &Path, to: &Path) -> String {
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (from, to) = (canonical(from), canonical(to));
    let (from, to): (Vec<Component>, Vec<Component>) = (from.components().collect(), to.components().collect());
    let shared = from.iter().zip(&to).take_while(|(left, right)| left == right).count();
    format!("{}{}/", "../".repeat(from.len() - shared), to[shared..].iter().collect::<PathBuf>().display())
}
