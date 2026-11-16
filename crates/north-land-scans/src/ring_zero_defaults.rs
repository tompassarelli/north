//! The harness/defaults.toml keys a lane changes that ring-0 code reads: a key whose `consumers`, at the lane's base or head, name a
//! file under ring_zero_paths and which the lane adds or changes, or removes while a ring-0 consumer of it remains (north#1650).
use std::collections::BTreeSet;
use std::path::Path;

use north_base::platform::harness::DEFAULTS;
use north_base::platform::trust_boundary;
use toml_edit::{DocumentMut, Item};

use crate::added_dependencies::shown;
use crate::git;

fn consumers(item: Option<&Item>) -> Vec<String> {
    item.and_then(|item| item.get("consumers")).and_then(Item::as_array).map(|list| list.iter().filter_map(toml_edit::Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

fn exists(directory: &Path, revision: &str, file: &str) -> bool {
    git::text(directory, &["cat-file", "-e", &format!("{revision}:{file}")]).is_some()
}

/// Each such key of the lane `fork..head`, as `harness/defaults.toml key KEY (read by FILES)`, or the file itself when its head does not parse.
pub(crate) fn changed(directory: &Path, fork: &str, head: &str, ring: &[&str]) -> Vec<String> {
    let parse = |revision: &str| git::text(directory, &["show", &format!("{revision}:{DEFAULTS}")]).unwrap_or_default().parse::<DocumentMut>();
    let before = parse(fork).unwrap_or_else(|_| DocumentMut::new());
    let Ok(after) = parse(head) else { return vec![format!("{DEFAULTS} does not parse")] };
    let keys: BTreeSet<&str> = before.as_table().iter().chain(after.as_table().iter()).map(|(key, _)| key).collect();
    let mut found = Vec::new();
    for key in keys {
        let (old, new) = (before.get(key), after.get(key));
        let read_by: Vec<String> = consumers(old).into_iter().chain(consumers(new)).collect::<BTreeSet<_>>().into_iter().collect();
        let read_by: Vec<&str> = trust_boundary::crossing_in(ring, &read_by);
        if read_by.is_empty() || old.map(shown) == new.map(shown) {
            continue;
        }
        if new.is_none() && read_by.iter().all(|file| !exists(directory, head, file)) {
            continue;
        }
        found.push(format!("{DEFAULTS} key {key} (read by {})", read_by.join(", ")));
    }
    found
}
