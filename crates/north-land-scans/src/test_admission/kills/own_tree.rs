use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

use super::super::super::git;

const PROFILE: &str = "target/debug";
const OWNER: &str = "owner";
const TREE: &str = "tree";

pub(super) struct OwnTree {
    origin: PathBuf,
    root: PathBuf,
    _scratch: tempfile::TempDir,
    _owner: fs::File,
}

impl OwnTree {
    pub(super) fn of(top: &Path, head: &str, sweeps: &Path) -> Result<Self, String> {
        reclaim(sweeps);
        let scratch = fs::create_dir_all(sweeps).and_then(|()| tempfile::Builder::new().prefix("sweep-").tempdir_in(sweeps)).map_err(|error| format!("{}: {error}", sweeps.display()))?;
        let owner = held(&scratch.path().join(OWNER), top).map_err(|error| format!("{}: {error}", scratch.path().display()))?;
        let root = scratch.path().join(TREE);
        let path = root.display().to_string();
        git::text(top, &["-c", "core.hooksPath=/dev/null", "clone", "--quiet", "--shared", "--no-checkout", ".", &path]).ok_or_else(|| format!("git clone --shared . {path} failed"))?;
        git::text(&root, &["-c", "core.hooksPath=/dev/null", "checkout", "--quiet", "--detach", head]).ok_or_else(|| format!("git checkout {head} in {path} failed"))?;
        let tree = Self { origin: top.to_path_buf(), root, _scratch: scratch, _owner: owner };
        tree.keep_times()?;
        if top.join(PROFILE).is_dir() {
            tree.mirror().map_err(|error| format!("copying {}: {error}", top.join(PROFILE).display()))?;
        }
        Ok(tree)
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
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

    fn mirror(&self) -> std::io::Result<()> {
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

    fn place(&self, relative: &Path, kind: fs::FileType) -> std::io::Result<()> {
        let (from, to) = (self.origin.join(relative), self.root.join(relative));
        if kind.is_symlink() {
            return std::os::unix::fs::symlink(fs::read_link(&from)?, &to);
        }
        fs::copy(&from, &to)?;
        stamp(&to, &fs::metadata(&from)?)
    }
}

fn stamp(path: &Path, like: &fs::Metadata) -> std::io::Result<()> {
    fs::File::options().write(true).open(path)?.set_modified(like.modified()?)
}

fn held(path: &Path, top: &Path) -> std::io::Result<fs::File> {
    let owner = fs::File::create(path)?;
    flock(&owner, FlockOperation::LockExclusive)?;
    std::io::Write::write_all(&mut &owner, format!("{}\n", top.display()).as_bytes())?;
    Ok(owner)
}

fn reclaim(sweeps: &Path) {
    let Ok(listing) = fs::read_dir(sweeps) else { return };
    for scratch in listing.flatten().map(|entry| entry.path()) {
        let Ok(owner) = fs::File::open(scratch.join(OWNER)) else { continue };
        if flock(&owner, FlockOperation::NonBlockingLockExclusive).is_ok() {
            let _ = fs::remove_dir_all(&scratch);
        }
    }
}
