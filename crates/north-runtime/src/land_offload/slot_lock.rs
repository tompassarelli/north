//! An offload slot's locks. Whoever owns a slot takes its `.lock` exclusively, then keeps a shared lock on its `.busy` file for as long
//! as it holds the slot. Whoever only asks whether a slot is held tests the `.busy` file and never the `.lock`, so the instant its test
//! holds a lock delays an owner's shared lock on `.busy` by that instant and never fails an owner's take of the slot. A job nested in the
//! slot its caller holds keeps a shared lock on the slot's `.nested` file until it ends, and an owner takes the slot only while no lock is
//! held there, so a nested job that outlives its caller keeps every new owner out of the slot it still writes.
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

const BUSY: &str = "busy";
const NESTED: &str = "nested";

/// A held slot: both locks, released together when it drops.
pub(crate) struct Hold {
    _lock: File,
    _busy: File,
}

fn busy(lock: &Path) -> PathBuf {
    lock.with_extension(BUSY)
}

fn unnested(lock: &Path) -> bool {
    OpenOptions::new().create(true).append(true).open(lock.with_extension(NESTED)).is_ok_and(|nested| flock(&nested, FlockOperation::NonBlockingLockExclusive).is_ok())
}

pub(crate) fn take(lock: &Path) -> Option<Hold> {
    let owned = OpenOptions::new().create(true).append(true).open(lock).ok()?;
    flock(&owned, FlockOperation::NonBlockingLockExclusive).ok()?;
    if !unnested(lock) {
        return None;
    }
    let busy = OpenOptions::new().create(true).append(true).open(busy(lock)).ok()?;
    flock(&busy, FlockOperation::LockShared).ok()?;
    Some(Hold { _lock: owned, _busy: busy })
}
