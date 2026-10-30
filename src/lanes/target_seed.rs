use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use toml_edit::DocumentMut;

const USAGE: &str = "usage: north target-seed [DIR]";
const HASH_LEN: usize = 16;

pub(crate) fn run(arguments: &[String]) -> u8 {
    let here = match arguments {
        [] => std::env::current_dir().unwrap_or_default(),
        [dir] => PathBuf::from(dir),
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    if std::env::var_os("CARGO_TARGET_DIR").is_some_and(|dir| !dir.is_empty()) {
        return 0;
    }
    match seed(&here) {
        Ok(Some(from)) => eprintln!("target-seed: copied {} into this checkout's target", from.display()),
        Ok(None) => {}
        Err(error) => eprintln!("target-seed: {error}"),
    }
    0
}

pub(crate) fn seed(here: &Path) -> io::Result<Option<PathBuf>> {
    let Some(workspace) = workspace_root(here) else { return Ok(None) };
    let target = workspace.join("target");
    let Some(checkout) = workspace.ancestors().find(|dir| dir.join(".git").exists()) else { return Ok(None) };
    let Ok(lock) = fs::read(workspace.join("Cargo.lock")) else { return Ok(None) };
    if target.exists() {
        return Ok(None);
    }
    let Some((_held, from)) = siblings(checkout)
        .into_iter()
        .filter_map(|sibling| Some(sibling.join(workspace.strip_prefix(checkout).ok()?)))
        .filter(|candidate| fs::read(candidate.join("Cargo.lock")).is_ok_and(|theirs| theirs == lock))
        .map(|candidate| candidate.join("target"))
        .filter(|target| target.join("CACHEDIR.TAG").is_file())
        .filter_map(|target| Some((hold(&target)?, target)))
        .max_by_key(|(_, target)| last_build(target))
    else {
        return Ok(None);
    };
    let staging = workspace.join(format!("target.seeding-{}", std::process::id()));
    let seeded = copy(&from, &staging).and_then(|()| forget_own_packages(&staging, &lock));
    if let Err(error) = seeded {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if fs::rename(&staging, &target).is_err() {
        fs::remove_dir_all(&staging)?;
        return Ok(None);
    }
    Ok(Some(from))
}

fn copy(from: &Path, to: &Path) -> io::Result<()> {
    let copied = Command::new("cp").arg("-a").arg("--reflink=auto").arg(from).arg(to).status()?;
    if copied.success() { Ok(()) } else { Err(io::Error::other(format!("could not copy {}", from.display()))) }
}

fn hold(target: &Path) -> Option<Vec<File>> {
    let locks = unit_dirs(target).into_iter().filter_map(|dir| File::open(dir.join(".cargo-lock")).ok());
    locks.map(|lock| match lock.try_lock_shared() {
        Ok(()) => Some(lock),
        Err(_) => None,
    }).collect()
}

fn last_build(target: &Path) -> Option<std::time::SystemTime> {
    unit_dirs(target).iter().flat_map(|dir| [dir.join("deps"), dir.join(".fingerprint")]).filter_map(|dir| fs::metadata(dir).ok()?.modified().ok()).max()
}

fn siblings(checkout: &Path) -> Vec<PathBuf> {
    let parent = checkout.parent().unwrap_or(checkout);
    let container = match parent.file_name().and_then(|name| name.to_str()) {
        Some("worktrees") => parent.parent().unwrap_or(parent),
        _ if checkout.file_name().is_some_and(|name| name == "main") => parent,
        _ => return Vec::new(),
    };
    let lanes = fs::read_dir(container.join("worktrees")).into_iter().flatten().flatten().map(|entry| entry.path());
    std::iter::once(container.join("main")).chain(lanes).filter(|sibling| sibling != checkout).collect()
}

fn workspace_root(here: &Path) -> Option<PathBuf> {
    let mut first = None;
    for dir in here.ancestors() {
        let Some(manifest) = fs::read_to_string(dir.join("Cargo.toml")).ok().and_then(|text| text.parse::<DocumentMut>().ok()) else { continue };
        if manifest.contains_key("workspace") {
            return Some(dir.to_owned());
        }
        first.get_or_insert_with(|| dir.to_owned());
    }
    first
}

// A package built from another checkout's sources reads as fresh when its files are older than the copied artifacts, so every Cargo.lock package without a registry or git source is rebuilt.
fn forget_own_packages(target: &Path, lock: &[u8]) -> io::Result<()> {
    let lock = String::from_utf8_lossy(lock).parse::<DocumentMut>().map_err(io::Error::other)?;
    let own = lock.get("package").and_then(|packages| packages.as_array_of_tables()).into_iter().flatten().filter(|package| package.get("source").is_none());
    let names: Vec<&str> = own.filter_map(|package| package.get("name")?.as_str()).collect();
    for profile in fingerprint_dirs(target) {
        for fingerprint in fs::read_dir(profile)?.flatten() {
            let file_name = fingerprint.file_name();
            let Some((name, hash)) = file_name.to_str().and_then(|text| text.rsplit_once('-')) else { continue };
            if hash.len() == HASH_LEN && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) && names.contains(&name) {
                fs::remove_dir_all(fingerprint.path())?;
            }
        }
    }
    Ok(())
}

fn fingerprint_dirs(target: &Path) -> Vec<PathBuf> {
    unit_dirs(target).into_iter().map(|dir| dir.join(".fingerprint")).filter(|dir| dir.is_dir()).collect()
}

fn unit_dirs(target: &Path) -> Vec<PathBuf> {
    let children = |dir: &Path| fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()).collect::<Vec<_>>();
    let nested = children(target).into_iter().flat_map(|child| children(&child));
    children(target).into_iter().chain(nested).filter(|dir| dir.join(".fingerprint").is_dir() || dir.join(".cargo-lock").is_file()).collect()
}
