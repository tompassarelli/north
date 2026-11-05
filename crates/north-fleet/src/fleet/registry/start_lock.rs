use std::fs::{File, OpenOptions};

use rustix::fs::{FlockOperation, flock};

use crate::error::{NorthError, NorthResult};

use super::Fleet;

pub(super) struct StartLock {
    _held: File,
}

impl Fleet {
    pub(super) fn start_lock(&self, name: &str) -> NorthResult<StartLock> {
        let file = OpenOptions::new().create(true).append(true).open(self.session_dir(name)?.join(".start.lock"))?;
        flock(&file, FlockOperation::LockExclusive).map_err(|error| NorthError::State(format!("{name} start lock: {error}")))?;
        Ok(StartLock { _held: file })
    }
}
