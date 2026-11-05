use std::fs::{self, File, OpenOptions};
use std::path::Path;

use rustix::fs::{FlockOperation, flock};

pub struct Lock {
    _held: File,
}

impl Lock {
    pub fn take(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        flock(&file, FlockOperation::LockExclusive)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Self { _held: file })
    }
}
