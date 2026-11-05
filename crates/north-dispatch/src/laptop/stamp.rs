use std::fs;
use std::path::Path;

use super::die::Die;
use crate::foundation::clock::Timestamp;

pub(super) fn iso(unix: i64) -> String {
    Timestamp::from_unix_seconds(unix).iso()
}

pub(super) fn parse(text: &str) -> Result<i64, Die> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds).ok_or_else(|| Die::new(1, format!("not a timestamp: {text}")))
}

pub(super) fn write_atomic(path: &Path, text: &str) -> Result<(), Die> {
    let io = |error: std::io::Error| Die::new(1, format!("{}: {error}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&temporary, text).map_err(io)?;
    fs::rename(&temporary, path).map_err(io)
}
