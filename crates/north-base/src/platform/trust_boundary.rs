//! The files whose change crosses a host's trust boundary, read from `trust_boundary_paths`, `two_accept_paths` and `landing_path_paths` in harness/defaults.toml: the one reading behind the test line a hand-in keeps before queueing and the review accepts a landing asks for (north#1036, north#1492).
use crate::platform::harness::{DEFAULTS, document};

const KEY: &str = "trust_boundary_paths";
const TWO_ACCEPTS: &str = "two_accept_paths";
pub const LANDING_PATH: &str = "landing_path_paths";

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

/// The paths harness/defaults.toml lists under `key`, `None` when it lists none.
pub fn listed(key: &str) -> Option<Vec<&'static str>> {
    listed_in(document(DEFAULTS), key)
}

fn listed_in<'a>(document: &'a toml_edit::DocumentMut, key: &str) -> Option<Vec<&'a str>> {
    let list = document.get(key).and_then(|entry| entry.get("value")).and_then(toml_edit::Item::as_array)?;
    Some(list.iter().filter_map(toml_edit::Value::as_str).collect())
}

/// The files of `files` that cross the boundary a harness/defaults.toml `document` lists, any of its three lists, such as `harness::document(DEFAULTS)` or the one main holds, `None` when it lists no trust_boundary_paths, which no caller may read as a change that crosses nothing.
pub fn crossing<'a>(document: &toml_edit::DocumentMut, files: &'a [String]) -> Option<Vec<&'a str>> {
    let boundary: Vec<&str> = listed_in(document, KEY)?.into_iter().chain([TWO_ACCEPTS, LANDING_PATH].into_iter().flat_map(|key| listed_in(document, key).unwrap_or_default())).collect();
    Some(crossing_in(&boundary, files))
}

/// The files of `files` in `two_accept_paths` of a harness/defaults.toml `document`, whose change needs two review accepts whatever its trailers say, `None` when it lists none.
pub fn needing_two<'a>(document: &toml_edit::DocumentMut, files: &'a [String]) -> Option<Vec<&'a str>> {
    Some(crossing_in(&listed_in(document, TWO_ACCEPTS)?, files))
}
