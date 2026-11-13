//! The files whose change crosses a host's trust boundary, read from `trust_boundary_paths` in harness/defaults.toml: the one reading behind the test line a hand-in keeps before queueing and the review accepts a landing asks for (north#1036).
use crate::platform::harness::{DEFAULTS, document};

const KEY: &str = "trust_boundary_paths";

fn names(boundary: &[&str], file: &str) -> bool {
    boundary.iter().any(|path| match path.strip_prefix("**/") {
        Some(name) => file == name || file.ends_with(&format!("/{name}")),
        None if path.ends_with('/') => file.starts_with(path),
        None => file == *path,
    })
}

/// The files of `files` that `boundary` names.
pub fn crossing_in<'a>(boundary: &[&str], files: &'a [String]) -> Vec<&'a str> {
    files.iter().map(String::as_str).filter(|file| names(boundary, file)).collect()
}

/// The files of `files` that cross the boundary of harness/defaults.toml, `None` when it lists none, which no caller may read as a change that crosses nothing.
pub fn crossing(files: &[String]) -> Option<Vec<&str>> {
    let list = document(DEFAULTS).get(KEY).and_then(|entry| entry.get("value")).and_then(toml_edit::Item::as_array)?;
    let boundary: Vec<&str> = list.iter().filter_map(toml_edit::Value::as_str).collect();
    Some(crossing_in(&boundary, files))
}
