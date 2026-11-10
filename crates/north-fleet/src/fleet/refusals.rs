//! The marks that keep placement and launches off an account its own client refused (subscription access
//! disabled, signed out, a revoked login): one file per account label under `account-refused`, whose first line
//! reads `SESSION ISO-TIME` and whose rest is the client's refusal text verbatim. Only a passing probe clears one.
use std::io;
use std::path::{Path, PathBuf};

const DIR: &str = "account-refused";

/// Where the marks of a fleet state root live.
pub fn directory(state_root: &Path) -> PathBuf {
    state_root.join(DIR)
}

/// The refusal text that keeps `label` unavailable, if its client refused it.
pub fn held(dir: &Path, label: &str) -> Option<String> {
    let mark = std::fs::read_to_string(dir.join(file(label)?)).ok()?;
    let text = mark.split_once('\n').map_or("", |(_, text)| text).trim();
    Some(if text.is_empty() { "the client refused it".to_owned() } else { text.to_owned() })
}

/// Marks `label` refused by its client, as `session` saw at `at` (an ISO time), keeping the refusal `text`.
pub fn mark(dir: &Path, label: &str, session: &str, at: &str, text: &str) -> io::Result<()> {
    let Some(file) = file(label) else { return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{label} is not an account label"))) };
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(file), format!("{session} {at}\n{}\n", text.trim()))
}

/// Clears the mark on `label` because a probe passed; whether one was held.
pub fn clear(dir: &Path, label: &str) -> io::Result<bool> {
    let Some(file) = file(label) else { return Ok(false) };
    match std::fs::remove_file(dir.join(file)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Every label under `dir` its client refused.
pub fn labels(dir: &Path) -> Vec<String> {
    let mut labels: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|entry| entry.path().is_file()).filter_map(|entry| entry.file_name().into_string().ok()).collect();
    labels.sort();
    labels
}

fn file(label: &str) -> Option<&str> {
    (!label.is_empty() && !label.contains(['/', '.'])).then_some(label)
}
