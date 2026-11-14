use std::path::Path;

use super::super::enqueue::git;

pub(super) fn in_lane(root: &Path, relative: &str, committed: &[String]) -> bool {
    let under = format!("{}/", relative.trim_end_matches('/'));
    committed.iter().any(|change| change == relative || change.starts_with(&under)) || tracked(root, relative)
}

pub(super) fn stageable(root: &Path, named: Vec<String>, relative: &[String]) -> Vec<String> {
    named.into_iter().zip(relative).filter(|(path, name)| Path::new(path).exists() || tracked(root, name)).map(|(path, _)| path).collect()
}

fn tracked(root: &Path, relative: &str) -> bool {
    let known = |arguments: &[&str]| git(root, &[&["--literal-pathspecs"][..], arguments, &["--", relative]].concat()).is_ok_and(|names| !names.is_empty());
    known(&["ls-files", "--cached"]) || known(&["ls-tree", "-r", "--name-only", "HEAD"])
}
