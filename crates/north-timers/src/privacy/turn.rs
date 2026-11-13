use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};
use sha2::{Digest, Sha256};

pub struct Turn {
    held: File,
    path: PathBuf,
}

impl Drop for Turn {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = flock(&self.held, FlockOperation::Unlock);
    }
}

/// This run's turn to judge `text` among the runs that share the verdict cache in `dir`, so two runs given one text ask the models once and the second reads the first's cached verdicts: `None` when another run holds it and `wait` is off, and a turn of nothing when the lock cannot be made.
pub fn take(dir: &Path, text: &str, wait: bool) -> Option<Option<Turn>> {
    let name: String = Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    let path = dir.join(format!("{name}.asking"));
    let opened = std::fs::create_dir_all(dir).and_then(|()| OpenOptions::new().create(true).truncate(false).write(true).open(&path));
    let Ok(held) = opened else { return Some(None) };
    if flock(&held, FlockOperation::NonBlockingLockExclusive).is_ok() {
        return Some(Some(Turn { held, path }));
    }
    if !wait {
        return None;
    }
    Some(flock(&held, FlockOperation::LockExclusive).is_ok().then_some(Turn { held, path }))
}
