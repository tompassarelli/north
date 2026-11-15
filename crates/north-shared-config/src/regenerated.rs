//! The files the landing candidate rewrites wholly: the `kind = "regenerate"` sets of harness/merge-sets.toml whose generator writes the whole file. The candidate reruns each one's command whenever a lane changes the file or its sources and folds the output into the lane, refusing the lane when it cannot, so the file that lands is the generator's, and a review accept of a patch need not cover it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const MERGE_SETS: &str = "harness/merge-sets.toml";

/// The `git diff` pathspec, after `--` and from the top of the work tree whatever the current directory, that leaves out every file regenerated both at `from` and at `to`, so neither side's own rules can widen what is left out; empty when nothing is.
pub fn left_out(directory: &Path, from: &str, to: &str) -> Vec<String> {
    let excluded: Vec<String> = at(directory, from).intersection(&at(directory, to)).map(|path| format!(":(top,exclude){path}")).collect();
    if excluded.is_empty() { Vec::new() } else { ["--".to_owned(), ":(top)".to_owned()].into_iter().chain(excluded).collect() }
}

fn at(directory: &Path, commit: &str) -> BTreeSet<String> {
    let shown = Command::new("git").arg("-C").arg(directory).args(["show", &format!("{commit}:{MERGE_SETS}")]).output();
    shown.ok().filter(|output| output.status.success()).map(|output| paths(&String::from_utf8_lossy(&output.stdout))).unwrap_or_default()
}

/// The `path` of each `[[set]]` of `text` the candidate rewrites wholly: `kind = "regenerate"` with a `north docs …-index` command and no `filled = true`. A set whose generator keeps any of the file's own lines (`cargo update`, or one that fills marked parts of prose) stays in the patch, since a hand edit of it can survive the candidate.
pub fn paths(text: &str) -> BTreeSet<String> {
    sets(text).into_iter().filter(rewritten).filter_map(|mut set| set.remove("path")).collect()
}

fn rewritten(set: &BTreeMap<String, String>) -> bool {
    let index = set.get("command").and_then(|command| command.strip_prefix("north docs ")).is_some_and(|name| name.ends_with("-index") && !name.contains(' '));
    set.get("kind").map(String::as_str) == Some("regenerate") && index && set.get("filled").is_none_or(|filled| filled == "false")
}

/// Each `[[set]]` table of `text` as its one-line `key = value` pairs, a string value without its quotes; each set writes the keys read here on one line each.
fn sets(text: &str) -> Vec<BTreeMap<String, String>> {
    let mut sets: Vec<BTreeMap<String, String>> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line == "[[set]]" {
            sets.push(BTreeMap::new());
        } else if let (Some(set), Some((key, value))) = (sets.last_mut(), line.split_once('=')) {
            let value = value.trim();
            set.insert(key.trim().to_owned(), value.strip_prefix('"').and_then(|text| text.strip_suffix('"')).unwrap_or(value).to_owned());
        }
    }
    sets
}
