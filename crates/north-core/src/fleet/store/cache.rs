use std::fs;
use std::io;
use std::path::Path;

use crate::error::NorthResult;

use super::SessionRow;

const CACHE: &str = ".fleet-sessions.json";

pub fn cache_sessions(root: &Path, rows: &[SessionRow]) -> NorthResult<()> {
    let staged = root.join(format!("{CACHE}.tmp"));
    fs::write(&staged, serde_json::to_vec(rows)?)?;
    fs::rename(staged, root.join(CACHE))?;
    Ok(())
}

pub fn cached_sessions(root: &Path) -> NorthResult<Option<Vec<SessionRow>>> {
    match fs::read(root.join(CACHE)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
