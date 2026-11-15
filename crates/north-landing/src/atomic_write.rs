use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent)?;
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(".{}.{}.{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)
}
