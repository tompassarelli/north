use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::select::Config;

const STAMP: &str = ".safe-push-deps";
const FIRST_INSTALL_REPOSITORIES: &[&str] = &["makgora", "wisp", "north"];

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

fn repository_name(top: &Path) -> String {
    let url = git(top, &["remote", "get-url", "origin"]).unwrap_or_default();
    let name = url.rsplit('/').next().unwrap_or_default();
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

fn installs_first(top: &Path) -> bool {
    FIRST_INSTALL_REPOSITORIES.contains(&repository_name(top).as_str())
        || (git(top, &["config", "--blob", "HEAD:.safe-push", "--get-all", "land.check"]).is_none() && Config::load(top, "HEAD").has_checks())
}

fn fingerprint(directory: &Path) -> String {
    let mut hasher = Sha256::new();
    for name in ["bun.lock", "package.json"] {
        if let Ok(bytes) = fs::read(directory.join(name)) {
            hasher.update(bytes);
        }
    }
    hasher.finalize()[..8].iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn sync_project(top: &Path, lock: &str, first_install: bool) -> Result<(), String> {
    let relative = Path::new(lock).parent().unwrap_or(Path::new(""));
    let directory: PathBuf = top.join(relative);
    let modules = directory.join("node_modules");
    if !modules.is_dir() {
        if !first_install || lock.matches('/').count() > 1 {
            return Ok(());
        }
        println!("safe-push: no node_modules — bun install --frozen-lockfile in {}", directory.display());
    }
    let want = fingerprint(&directory);
    let stamp = modules.join(STAMP);
    if fs::read_to_string(&stamp).is_ok_and(|text| text.trim_end_matches('\n') == want) {
        return Ok(());
    }
    if modules.is_symlink() {
        let target = fs::read_link(&modules).map(|target| target.display().to_string()).unwrap_or_default();
        return Err(format!(
            "safe-push: {} is a symlink to {target} and {lock} no longer matches it — bun install would rewrite that checkout; replace the symlink with its own install; NOT pushed.",
            modules.display()
        ));
    }
    println!("safe-push: {lock} changed — bun install --frozen-lockfile in {}", directory.display());
    let status = Command::new("bun").args(["install", "--frozen-lockfile"]).current_dir(&directory).status();
    if !status.is_ok_and(|status| status.success()) {
        return Err(format!("safe-push: bun install --frozen-lockfile failed in {}", directory.display()));
    }
    fs::write(&stamp, format!("{want}\n")).map_err(|error| format!("safe-push: {}: {error}", stamp.display()))
}

pub fn sync(directory: &Path) -> Result<(), String> {
    let top = PathBuf::from(git(directory, &["rev-parse", "--show-toplevel"]).ok_or("safe-push: not inside a git repository")?);
    let first_install = installs_first(&top);
    let locks = git(&top, &["ls-files", "--", "bun.lock", "*/bun.lock"]).unwrap_or_default();
    locks.lines().try_for_each(|lock| sync_project(&top, lock, first_install))
}

pub fn run(directory: &Path) -> u8 {
    match sync(directory) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
