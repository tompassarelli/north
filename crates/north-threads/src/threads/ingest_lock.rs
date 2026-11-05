use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};

pub const RETRY: i32 = 75;
const POLL: Duration = Duration::from_millis(50);

pub struct IngestLock {
    _file: File,
}

pub fn acquire(path: &Path, wait: Duration) -> io::Result<Option<IngestLock>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let deadline = Instant::now() + wait;
    loop {
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(Some(IngestLock { _file: file })),
            Err(rustix::io::Errno::WOULDBLOCK) if Instant::now() < deadline => sleep(POLL),
            Err(rustix::io::Errno::WOULDBLOCK) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
}
