use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

const LOCK: &str = "lander.lock";

pub struct Hold {
    _file: File,
}

pub fn path(queue: &Path) -> PathBuf {
    queue.join(LOCK)
}

pub fn holder(queue: &Path) -> String {
    let named = fs::read_to_string(path(queue)).unwrap_or_default();
    let named = named.trim();
    if named.is_empty() { "a lander that has not named itself yet".to_owned() } else { named.to_owned() }
}

pub fn take(queue: &Path, serving: &Path) -> Result<Hold, String> {
    fs::create_dir_all(queue).map_err(|error| format!("cannot create {}: {error}", queue.display()))?;
    let lock = path(queue);
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock).map_err(|error| format!("cannot open {}: {error}", lock.display()))?;
    if flock(&file, FlockOperation::NonBlockingLockExclusive).is_err() {
        return Err(format!("queue {} is already served by {}; refusing to serve it twice", queue.display(), holder(queue)));
    }
    let named = format!("pid {} serving {}\n", std::process::id(), serving.display());
    file.set_len(0).and_then(|()| file.write_all(named.as_bytes())).map_err(|error| format!("cannot name the holder in {}: {error}", lock.display()))?;
    Ok(Hold { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_hold_on_one_queue_is_refused_naming_the_first_and_is_granted_once_it_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let queue = dir.path().join("queue");
        let first = take(&queue, Path::new("/code/north/main")).unwrap();
        let refused = take(&queue, Path::new("/code/north/other")).err().unwrap();
        assert!(refused.contains(&format!("pid {} serving /code/north/main", std::process::id())), "{refused}");
        assert_eq!(holder(&queue), format!("pid {} serving /code/north/main", std::process::id()));
        drop(first);
        let second = take(&queue, Path::new("/code/north/other"));
        assert!(second.is_ok());
        assert_eq!(holder(&queue), format!("pid {} serving /code/north/other", std::process::id()));
    }
}
