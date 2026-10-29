use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};
use serde_json::Value;

use crate::error::{NorthError, NorthResult};

use super::configuration;
use super::paths::agents_root;

pub(super) fn with_lock<T>(operation: impl FnOnce() -> NorthResult<T>) -> NorthResult<T> {
    let root = agents_root()?;
    fs::create_dir_all(&root)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(".lock"))?;
    flock(&lock, FlockOperation::LockExclusive).map_err(|error| {
        NorthError::Configuration(format!("cannot acquire agent activation lock: {error}"))
    })?;
    operation()
}

pub(super) fn current_generation_path() -> NorthResult<PathBuf> {
    let current = agents_root()?.join("current");
    fs::canonicalize(&current).map_err(|error| {
        NorthError::Configuration(format!(
            "cannot resolve current activation {}: {error}",
            current.display()
        ))
    })
}

pub(super) fn read_current() -> NorthResult<Value> {
    read_json(&agents_root()?.join("current/activation.json"))
}

pub(super) fn read_json(path: &Path) -> NorthResult<Value> {
    let bytes = fs::read(path).map_err(|error| {
        NorthError::Configuration(format!("cannot read {}: {error}", path.display()))
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn write_json(path: &Path, value: &Value) -> NorthResult<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn copy_tree(source: &Path, target: &Path) -> NorthResult<()> {
    if source.is_dir() {
        fs::create_dir_all(target)?;
        let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if source.is_file() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
    } else {
        return configuration(format!(
            "refusing unsupported source type: {}",
            source.display()
        ));
    }
    Ok(())
}

pub(super) fn replace_copy(source: &Path, target: &Path) -> NorthResult<()> {
    if target.is_dir() {
        fs::remove_dir_all(target)?;
    } else if target.exists() || target.is_symlink() {
        fs::remove_file(target)?;
    }
    copy_tree(source, target)
}

pub(super) fn atomic_symlink(link: &Path, target: &Path) -> NorthResult<()> {
    let parent = link
        .parent()
        .ok_or_else(|| NorthError::Configuration("link has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".north-link-{}.tmp", std::process::id()));
    if temporary.exists() || temporary.is_symlink() {
        fs::remove_file(&temporary)?;
    }
    symlink(target, &temporary)?;
    fs::rename(temporary, link)?;
    Ok(())
}
