//! The identity a spare worker learns when a launch takes it: the client started before its session had a name, so the
//! launch writes the name into a binding file in the spare's held directory, where a worker cannot write, and the
//! worker-sandbox prefix every hook, shell command and tool call passes through applies it before anything reads the
//! session's variables.
use std::path::PathBuf;

/// The variable naming a spare's binding file.
pub const ENV: &str = "NORTH_FLEET_BINDING";
/// The variables a binding may set: the session's name, its actor and its capacity need.
pub const KEYS: [&str; 3] = ["NORTH_FLEET_SESSION", "THREADS_ACTOR", "AGENT_CAPACITY_NEED"];

/// The bound variables whose value in this process's environment is not yet the bound one; empty outside a taken spare.
pub fn unapplied() -> Vec<(String, String)> {
    let Some(file) = std::env::var_os(ENV).filter(|file| !file.is_empty()).map(PathBuf::from) else { return Vec::new() };
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, value)| KEYS.contains(key) && std::env::var(key).ok().as_deref() != Some(*value))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}
