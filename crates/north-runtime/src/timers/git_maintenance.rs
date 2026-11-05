use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::home;

pub(super) fn repositories(code: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = fs::read_dir(code)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let repo = entry.path().join("main");
            (name != "archive" && repo.join(".git").exists()).then_some((name, repo))
        })
        .collect();
    found.sort_by(|left, right| left.1.cmp(&right.1));
    found
}

pub(super) fn maintenance_argv(name: &str, repo: &Path) -> Vec<String> {
    let repo = repo.to_string_lossy();
    [
        "capacity", "host", "run", "--class", "moderate", "--owner", &format!("git-maintenance:{name}"), "--timeout-seconds", "3600",
        "--", "git", "-C", repo.as_ref(), "-c", "pack.threads=2", "maintenance", "run",
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn run(_arguments: &[String]) -> u8 {
    let mut status = 0;
    for (name, repo) in repositories(&home().join("code")) {
        println!("git-maintenance-nightly: {}", repo.display());
        match Command::new(north_base::platform::north_binary::north_binary()).args(maintenance_argv(&name, &repo)).status() {
            Ok(code) if code.success() => {}
            Ok(code) => {
                eprintln!("git-maintenance-nightly: {} failed: {code}", repo.display());
                status = 1;
            }
            Err(error) => {
                eprintln!("git-maintenance-nightly: {} failed to start: {error}", repo.display());
                status = 1;
            }
        }
    }
    status
}
