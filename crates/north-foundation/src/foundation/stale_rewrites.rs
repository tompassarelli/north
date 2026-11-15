use std::path::{Path, PathBuf};

pub const FILE: &str = ".local/state/north/stale-rewrites.tsv";

pub fn file(home: &Path) -> PathBuf {
    home.join(FILE)
}

pub fn read(file: &Path) -> Result<Vec<(String, String)>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(text.lines().filter_map(|line| line.split_once('\t')).map(|(closed, successor)| (closed.to_owned(), successor.to_owned())).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {error}", file.display())),
    }
}
