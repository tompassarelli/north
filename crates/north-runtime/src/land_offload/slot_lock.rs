//! An offload slot's two locks. Whoever owns a slot takes its `.lock` exclusively, then keeps a shared lock on its `.busy` file for as long
//! as it holds the slot. Whoever only asks whether a slot is held tests the `.busy` file and never the `.lock`, so the instant its test
//! holds a lock delays an owner's shared lock on `.busy` by that instant and never fails an owner's take of the slot.
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

const BUSY: &str = "busy";

/// A held slot: both locks, released together when it drops.
pub(crate) struct Hold {
    _lock: File,
    _busy: File,
}

fn busy(lock: &Path) -> PathBuf {
    lock.with_extension(BUSY)
}

pub(crate) fn held(lock: &Path) -> bool {
    File::open(busy(lock)).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockExclusive).is_err())
}

pub(crate) fn take(lock: &Path) -> Option<Hold> {
    let owned = OpenOptions::new().create(true).append(true).open(lock).ok()?;
    flock(&owned, FlockOperation::NonBlockingLockExclusive).ok()?;
    let busy = OpenOptions::new().create(true).append(true).open(busy(lock)).ok()?;
    flock(&busy, FlockOperation::LockShared).ok()?;
    Some(Hold { _lock: owned, _busy: busy })
}
