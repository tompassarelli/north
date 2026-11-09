use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

use rustix::fs::{FlockOperation, flock};

use crate::error::{NorthError, NorthResult};

use super::Fleet;

const LOCKS: &str = ".start-locks";

thread_local! {
    static HELD: RefCell<BTreeSet<PathBuf>> = const { RefCell::new(BTreeSet::new()) };
}

/// The lock on a session's name that every start, stop, archive and take-over of it holds. It lies beside the session directories rather than in one, so a take-over that archives the directory and makes a new one keeps one lock throughout; a nested take on this thread, such as a take-over's archive stopping the old host, shares it.
pub(super) struct StartLock(Option<(PathBuf, File)>);

impl Drop for StartLock {
    fn drop(&mut self) {
        if let Some((path, _)) = &self.0 {
            HELD.with_borrow_mut(|held| held.remove(path));
        }
    }
}

impl Fleet {
    pub(super) fn start_lock(&self, name: &str) -> NorthResult<StartLock> {
        self.session_dir(name)?;
        let locks = self.root.join(LOCKS);
        let path = locks.join(name);
        if HELD.with_borrow(|held| held.contains(&path)) {
            return Ok(StartLock(None));
        }
        fs::create_dir_all(&locks)?;
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        flock(&file, FlockOperation::LockExclusive).map_err(|error| NorthError::State(format!("{name} start lock: {error}")))?;
        HELD.with_borrow_mut(|held| held.insert(path.clone()));
        Ok(StartLock(Some((path, file))))
    }
}
