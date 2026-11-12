//! The tracked files a repository's own build writes: the ones the shipped commit's `harness/merge-sets.toml` declares
//! as regenerated from per-unit sources. A lane may not commit them, so a build on a slot rewrites them from the lane's
//! sources, and the slot's tree check leaves out exactly these and no other tracked file.
use std::path::Path;
use std::process::Command;

use north_base::foundation::offload;

const DECLARED: &str = "harness/merge-sets.toml";

/// The declared paths of `head`, one a line; none when the commit declares none or its declaration cannot be read, which
/// leaves the tree check as strict as it is without one.
pub(super) fn paths(checkout: &Path, head: &str) -> String {
    let shown = Command::new(offload::git()).current_dir(checkout).args(["show", &format!("{head}:{DECLARED}")]).output();
    shown.ok().filter(|shown| shown.status.success()).map(|shown| declared(&String::from_utf8_lossy(&shown.stdout)).join("\n")).unwrap_or_default()
}

fn declared(text: &str) -> Vec<String> {
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    let sets = document.get("set").and_then(toml_edit::Item::as_array_of_tables);
    let field = |set: &toml_edit::Table, key: &str| set.get(key).and_then(toml_edit::Item::as_str).map(str::to_owned);
    sets.into_iter().flatten().filter(|set| field(set, "kind").as_deref() == Some("regenerate")).filter_map(|set| field(set, "path")).collect()
}
