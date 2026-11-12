//! The tracked files a repository's own build writes: the ones `harness/merge-sets.toml` declares as regenerated from
//! per-unit sources. A lane may not commit them, so a build on a slot rewrites them from the lane's sources, and the
//! slot's tree check leaves out exactly these and no other tracked file. The declaration is read at the base the run
//! names, never at the shipped commit: a lane that declares a file regenerated gets that only once it has landed, so no
//! lane can excuse a file its own run rewrites.
use std::process::Command;

use north_base::foundation::offload;

use super::Land;

const DECLARED: &str = "harness/merge-sets.toml";

/// The paths the run's base declares, one a line; none for a run that names no base or its own head as the base, and
/// none when the base declares none or its declaration cannot be read, which leaves the tree check as strict as it is
/// without one.
pub(super) fn paths(land: &Land) -> String {
    let Some(base) = land.base.as_ref().filter(|base| **base != land.head) else { return String::new() };
    let shown = Command::new(offload::git()).current_dir(land.checkout).args(["show", &format!("{}:{DECLARED}", base.as_str())]).output();
    shown.ok().filter(|shown| shown.status.success()).map(|shown| declared(&String::from_utf8_lossy(&shown.stdout)).join("\n")).unwrap_or_default()
}

fn declared(text: &str) -> Vec<String> {
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    let sets = document.get("set").and_then(toml_edit::Item::as_array_of_tables);
    let field = |set: &toml_edit::Table, key: &str| set.get(key).and_then(toml_edit::Item::as_str).map(str::to_owned);
    sets.into_iter().flatten().filter(|set| field(set, "kind").as_deref() == Some("regenerate")).filter_map(|set| field(set, "path")).collect()
}
