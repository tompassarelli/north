use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

const LOCK_STALE: Duration = Duration::from_secs(2);
const LOCK_PATIENCE: Duration = Duration::from_secs(2);

pub(crate) fn pause(capacity: &Path, ids: &BTreeSet<String>) {
    rewrite(capacity, ids, |lease, now| {
        if !lease["frozenAt"].is_null() {
            return false;
        }
        lease["frozenAt"] = now.into();
        true
    });
}

pub(crate) fn resume(capacity: &Path, ids: &BTreeSet<String>) {
    rewrite(capacity, ids, |lease, now| {
        let Some(frozen_at) = lease.as_object_mut().and_then(|fields| fields.remove("frozenAt")).and_then(|at| at.as_u64()) else {
            return false;
        };
        if let Some(expires_at) = lease["expiresAt"].as_u64() {
            lease["expiresAt"] = (expires_at + now.saturating_sub(frozen_at)).into();
        }
        true
    });
}

fn rewrite(capacity: &Path, ids: &BTreeSet<String>, change: impl Fn(&mut Value, u64) -> bool) {
    if ids.is_empty() {
        return;
    }
    let Some(_lock) = Lock::take(capacity) else { return };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64);
    for path in ids.iter().map(|id| capacity.join("leases").join(format!("{id}.json"))) {
        let Some(mut lease) = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else { continue };
        if change(&mut lease, now) {
            let _ = replace(&path, &format!("{lease}\n"));
        }
    }
}

fn replace(path: &Path, text: &str) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temporary)?.write_all(text.as_bytes())?;
    fs::rename(temporary, path)
}

struct Lock(PathBuf);

impl Lock {
    fn take(capacity: &Path) -> Option<Self> {
        let path = capacity.join("lock");
        let deadline = Instant::now() + LOCK_PATIENCE;
        while Instant::now() < deadline {
            match fs::create_dir(&path) {
                Ok(()) => return Some(Self(path)),
                Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => return None,
                Err(_) => {}
            }
            if fs::metadata(&path).and_then(|metadata| metadata.modified()).is_ok_and(|at| at.elapsed().is_ok_and(|age| age > LOCK_STALE)) {
                let _ = fs::remove_dir_all(&path);
                continue;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

