use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::verdict_cache::path_set::Kind;

const EXECUTABLE_BITS: u32 = 0o111;

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn content(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.is_dir() {
        return Some(format!("directory {}", fs::canonicalize(path).ok()?.display()));
    }
    fs::read(path).ok().map(|bytes| digest(&bytes))
}

fn listing(path: &Path) -> Option<String> {
    let mut names: Vec<String> = fs::read_dir(path).ok()?.map(|entry| {
        let entry = entry.ok()?;
        let mode = entry.path().symlink_metadata().ok()?.permissions().mode();
        Some(format!("{mode:o} {}", entry.file_name().to_str()?))
    }).collect::<Option<_>>()?;
    names.sort();
    Some(digest(names.join("\n").as_bytes()))
}

pub fn answer(kind: Kind, path: &str) -> Option<String> {
    let path = Path::new(path);
    match kind {
        Kind::Absent => Some(if path.exists() { "present" } else { "absent" }.to_owned()),
        Kind::Content => content(path),
        Kind::Exec => Some(format!("{} {}", content(path)?, fs::metadata(path).ok()?.permissions().mode() & EXECUTABLE_BITS != 0)),
        Kind::Listing => listing(path),
        Kind::Unreusable => None,
    }
}
