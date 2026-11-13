//! The per-unit tree behind a generated registry: the files a renderer joins, in name order.
use std::fs;
use std::path::{Path, PathBuf};

pub fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// The files under `dir` whose extension is `extension`, in name order, skipping `_` and `.` names.
pub fn files(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|error| format!("{}: {error}", dir.display())))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| !name.starts_with('_') && !name.starts_with('.')))
        .collect())
}
