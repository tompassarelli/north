use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Shell {
    pub name: String,
    pub worktree: PathBuf,
    pub also: usize,
}

pub fn worktrees(code: &Path) -> Vec<(String, PathBuf)> {
    let mut containers: Vec<PathBuf> = fs::read_dir(code).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    containers.sort();
    let mut found = Vec::new();
    for container in containers {
        let main = container.join("main");
        if !main.join(".git").exists() {
            continue;
        }
        let repo = container.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let listed = Command::new("git").arg("-C").arg(&main).args(["worktree", "list", "--porcelain"]).output().map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
        found.extend(parse_worktrees(&listed).into_iter().filter(|path| path.is_dir()).map(|path| (repo.clone(), path)));
    }
    found
}

pub fn parse_worktrees(porcelain: &str) -> Vec<PathBuf> {
    porcelain
        .split("\n\n")
        .filter(|block| !block.lines().any(|line| line == "bare" || line.starts_with("prunable")))
        .filter_map(|block| block.lines().find_map(|line| line.strip_prefix("worktree ")))
        .map(PathBuf::from)
        .collect()
}

fn key(worktree: &Path) -> Option<String> {
    let flake = fs::read_to_string(worktree.join("flake.nix")).ok()?;
    if !flake.contains("devShell") {
        return None;
    }
    let listed = Command::new("git").arg("-C").arg(worktree).args(["ls-files", "-z", "--", "*.nix", "flake.lock", "rust-toolchain*", "bin/cargo"]).output().ok()?;
    let mut hash = Sha256::new();
    for name in listed.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()) {
        hash.update(name);
        hash.update(fs::read(worktree.join(String::from_utf8_lossy(name).as_ref())).unwrap_or_default());
    }
    Some(hash.finalize().iter().take(8).map(|byte| format!("{byte:02x}")).collect())
}

pub fn live(code: &Path) -> Vec<Shell> {
    let mut shells: Vec<Shell> = Vec::new();
    for (repo, worktree) in worktrees(code) {
        let Some(key) = key(&worktree) else {
            continue;
        };
        let name = format!("{repo}-{key}");
        match shells.iter_mut().find(|shell| shell.name == name) {
            Some(shell) => shell.also += 1,
            None => shells.push(Shell { name, worktree, also: 0 }),
        }
    }
    shells
}
