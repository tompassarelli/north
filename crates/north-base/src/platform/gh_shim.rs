use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const MARKER: &str = "north gh-session";

pub fn is_shim(path: &Path) -> bool {
    let mut head = [0; 4096];
    let read = std::fs::File::open(path).and_then(|mut file| file.read(&mut head)).unwrap_or(0);
    String::from_utf8_lossy(&head[..read]).contains(MARKER)
}

pub fn first_on_path(path: &str) -> Option<PathBuf> {
    candidates(path).next()
}

fn candidates(path: &str) -> impl Iterator<Item = PathBuf> + '_ {
    path.split(':').filter(|directory| !directory.is_empty()).map(|directory| Path::new(directory).join("gh")).filter(|candidate| executable(candidate))
}

fn executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}


pub fn real_on_path(path: &str) -> Option<PathBuf> {
    candidates(path).find(|candidate| !is_shim(candidate))
}
