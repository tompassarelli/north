use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::super::super::git;

const PROFILE: &str = "target/debug";
const LINKED: [&str; 3] = ["deps", "incremental", "examples"];
const OWNER: &str = "owner";
const TREE: &str = "tree";

pub(super) struct OwnTree {
    origin: PathBuf,
    root: PathBuf,
    linked: Vec<Linked>,
    _scratch: tempfile::TempDir,
}

struct Linked {
    origin: PathBuf,
    inode: u64,
    len: u64,
    modified: Option<SystemTime>,
}

impl OwnTree {
    pub(super) fn of(top: &Path, head: &str, sweeps: &Path) -> Result<Self, String> {
        reclaim(sweeps);
        let scratch = fs::create_dir_all(sweeps).and_then(|()| tempfile::Builder::new().prefix("sweep-").tempdir_in(sweeps)).map_err(|error| format!("{}: {error}", sweeps.display()))?;
        fs::write(scratch.path().join(OWNER), format!("{}\n{}\n", std::process::id(), top.display())).map_err(|error| format!("{}: {error}", scratch.path().display()))?;
        let root = scratch.path().join(TREE);
        let path = root.display().to_string();
        git::text(top, &["-c", "core.hooksPath=/dev/null", "worktree", "add", "--quiet", "--detach", &path, head]).ok_or_else(|| format!("git worktree add {path} {head} failed"))?;
        let mut tree = Self { origin: top.to_path_buf(), root, linked: Vec::new(), _scratch: scratch };
        tree.keep_times()?;
        if top.join(PROFILE).is_dir() {
            tree.mirror().map_err(|error| format!("copying {}: {error}", top.join(PROFILE).display()))?;
        }
        Ok(tree)
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn written_through(&self) -> Vec<PathBuf> {
        let written: Vec<PathBuf> = self
            .linked
            .iter()
            .filter(|linked| fs::metadata(&linked.origin).is_ok_and(|now| now.ino() == linked.inode && (now.len() != linked.len || now.modified().ok() != linked.modified)))
            .map(|linked| linked.origin.clone())
            .collect();
        for path in &written {
            let _ = fs::remove_file(path);
        }
        written
    }

    fn keep_times(&self) -> Result<(), String> {
        let listed = git::bytes(&self.origin, &["ls-files", "-z"]).ok_or("git ls-files failed")?;
        let changed = git::bytes(&self.origin, &["diff", "--name-only", "--no-renames", "-z", "HEAD"]).ok_or("git diff HEAD failed")?;
        let changed: Vec<&[u8]> = changed.split(|byte| *byte == 0).collect();
        for path in listed.split(|byte| *byte == 0).filter(|path| !path.is_empty() && !changed.contains(path)).map(|path| Path::new(OsStr::from_bytes(path))) {
            let Ok(meta) = fs::symlink_metadata(self.origin.join(path)) else { continue };
            if meta.is_file() {
                stamp(&self.root.join(path), &meta).map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        Ok(())
    }

    fn mirror(&mut self) -> std::io::Result<()> {
        let mut pending = vec![PathBuf::from(PROFILE)];
        while let Some(directory) = pending.pop() {
            fs::create_dir_all(self.root.join(&directory))?;
            for entry in fs::read_dir(self.origin.join(&directory))? {
                let entry = entry?;
                let relative = directory.join(entry.file_name());
                if entry.file_type()?.is_dir() {
                    pending.push(relative);
                } else if let Err(error) = self.place(&relative, entry.file_type()?)
                    && error.kind() != ErrorKind::NotFound
                {
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn place(&mut self, relative: &Path, kind: fs::FileType) -> std::io::Result<()> {
        let (from, to) = (self.origin.join(relative), self.root.join(relative));
        if kind.is_symlink() {
            return std::os::unix::fs::symlink(fs::read_link(&from)?, &to);
        }
        if !linked(relative) {
            fs::copy(&from, &to)?;
            return stamp(&to, &fs::metadata(&from)?);
        }
        fs::hard_link(&from, &to)?;
        let meta = fs::metadata(&to)?;
        self.linked.push(Linked { origin: from, inode: meta.ino(), len: meta.len(), modified: meta.modified().ok() });
        Ok(())
    }
}

impl Drop for OwnTree {
    fn drop(&mut self) {
        let _ = git::bytes(&self.origin, &["worktree", "remove", "--force", &self.root.display().to_string()]);
    }
}

fn stamp(path: &Path, like: &fs::Metadata) -> std::io::Result<()> {
    fs::File::options().write(true).open(path)?.set_modified(like.modified()?)
}

fn linked(relative: &Path) -> bool {
    let Ok(inner) = relative.strip_prefix(PROFILE) else { return false };
    let name = inner.file_name().and_then(OsStr::to_str).unwrap_or_default();
    let rewritten = name.starts_with('.') || name.ends_with(".d") || name.ends_with(".o");
    let first = inner.components().next().and_then(|first| first.as_os_str().to_str()).unwrap_or_default();
    !rewritten && (inner.components().count() == 1 || LINKED.contains(&first))
}

fn reclaim(sweeps: &Path) {
    let Ok(listing) = fs::read_dir(sweeps) else { return };
    for scratch in listing.flatten().map(|entry| entry.path()) {
        let Ok(owner) = fs::read_to_string(scratch.join(OWNER)) else { continue };
        let mut lines = owner.lines();
        let (Some(pid), Some(origin)) = (lines.next(), lines.next()) else { continue };
        if Path::new("/proc").join(pid).exists() {
            continue;
        }
        let _ = git::bytes(Path::new(origin), &["worktree", "remove", "--force", &scratch.join(TREE).display().to_string()]);
        let _ = fs::remove_dir_all(&scratch);
    }
}
