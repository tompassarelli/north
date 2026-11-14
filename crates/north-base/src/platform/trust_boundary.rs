//! The files whose change crosses a host's trust boundary, read from `trust_boundary_paths` and `two_accept_paths` in harness/defaults.toml: the one reading behind the test line a hand-in keeps before queueing and the review accepts a landing asks for (north#1036, north#1492).
use crate::platform::harness::{DEFAULTS, document};

const KEY: &str = "trust_boundary_paths";
const TWO_ACCEPTS: &str = "two_accept_paths";

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

fn listed(key: &str) -> Option<Vec<&'static str>> {
    let list = document(DEFAULTS).get(key).and_then(|entry| entry.get("value")).and_then(toml_edit::Item::as_array)?;
    Some(list.iter().filter_map(toml_edit::Value::as_str).collect())
}

/// The files of `files` that cross the boundary of harness/defaults.toml, either list of it, `None` when it lists no trust_boundary_paths, which no caller may read as a change that crosses nothing.
pub fn crossing(files: &[String]) -> Option<Vec<&str>> {
    let mut boundary = listed(KEY)?;
    boundary.extend(listed(TWO_ACCEPTS).unwrap_or_default());
    Some(crossing_in(&boundary, files))
}

/// The files of `files` in `two_accept_paths`, whose change needs two review accepts whatever its trailers say, `None` when harness/defaults.toml lists none.
pub fn needing_two(files: &[String]) -> Option<Vec<&str>> {
    Some(crossing_in(&listed(TWO_ACCEPTS)?, files))
}
