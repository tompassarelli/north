use crate::foundation::clock::Timestamp;
use rustix::fs::{FlockOperation, flock};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) fn now() -> f64 {
    Timestamp::now().unix_seconds_f64()
}
pub(super) fn directory() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("agents")
}
pub(super) fn load(path: &Path) -> Value {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({}))
}
pub(super) fn save(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("state file has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut pending, &serde_json::to_vec(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    pending.as_file().sync_all().map_err(|e| e.to_string())?;
    pending.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn lock(directory: &Path, name: &str, wait: Option<f64>) -> Result<File, String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(directory.join(name))
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(file),
            Err(error) if error == rustix::io::Errno::WOULDBLOCK => {
                if wait.is_some_and(|wait| start.elapsed().as_secs_f64() >= wait) {
                    return Err("pace controller is busy".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}
pub(super) fn append(path: &Path, value: &Value) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("audit has no directory")?)
        .map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    flock(&file, FlockOperation::LockExclusive).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 0 {
        file.seek(SeekFrom::End(-1)).map_err(|e| e.to_string())?;
        let mut last = [0];
        file.read_exact(&mut last).map_err(|e| e.to_string())?;
        if last[0] != b'\n' {
            file.write_all(b"\n").map_err(|e| e.to_string())?;
        }
    }
    writeln!(file, "{value}").map_err(|e| e.to_string())
}
pub(super) fn rows(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}
pub(super) fn number(value: &Value, fallback: f64) -> f64 {
    value.as_f64().unwrap_or(fallback)
}
pub(super) fn default(name: &str) -> f64 {
    crate::pace::limits::default(name)
}
