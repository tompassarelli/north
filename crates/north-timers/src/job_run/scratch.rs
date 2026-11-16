use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

/// The job's own scratch directory `ROOT/NAME`, which the host's configuration names through `--scratch ROOT` and the job sees as its `TMPDIR`. It starts empty, since whatever a killed run of the same job left there is that job's alone, and is removed when the job ends; on the laptop ROOT is memory, so neither a job's scratch nor its leftovers reach the drive (north#1553).
pub(super) struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    pub(super) fn fresh(root: &Path, name: &str) -> Result<Self, String> {
        let plain = Path::new(name).components().collect::<Vec<_>>();
        if !root.is_absolute() || !matches!(plain.as_slice(), [Component::Normal(_)]) {
            return Err(format!("job-run: --scratch needs an absolute directory and a job name with no slash, not {} and {name}", root.display()));
        }
        let directory = root.join(name);
        let made = fs::DirBuilder::new().recursive(true).mode(0o700).create(root).and_then(|()| fs::set_permissions(root, fs::Permissions::from_mode(0o700)));
        made.and_then(|()| empty(&directory)).and_then(|()| fs::DirBuilder::new().mode(0o700).create(&directory)).map_err(|error| format!("job-run: scratch {}: {error}", directory.display()))?;
        Ok(Self { directory })
    }

    pub(super) fn path(&self) -> &Path {
        &self.directory
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = empty(&self.directory) {
            eprintln!("job-run: remove {}: {error}", self.directory.display());
        }
    }
}

fn empty(directory: &Path) -> io::Result<()> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}
