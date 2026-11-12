//! The tree an after-landing verdict was computed on, named in the sheriff's request so a red verdict on any other tree reverts nothing. It is read from the record the run's own offload clients write beside its log, never from the log, which a check's output can fill with any line.
use std::fs;
use std::path::{Path, PathBuf};

use north_base::foundation::offload::UNPROVED;

const EXTENSION: &str = "trees";
const RUN: &str = "run ";
const OFFLOADED: &str = "offloaded";
const HERE: &str = "here";

/// The record of the run whose log is `log`, which the lander creates for each run and whose offload clients append to.
pub fn record(log: &Path) -> PathBuf {
    log.with_extension(EXTENSION)
}

/// The first line of a run's record: the commit it checks, and whether it ships its checks to a build host, whose every run must then be recorded.
pub fn header(commit: &str, offloaded: bool) -> String {
    format!("{RUN}{commit} {}\n", if offloaded { OFFLOADED } else { HERE })
}

fn proved(line: &str) -> Option<&str> {
    let (tree, _) = line.split_once(' ')?;
    (matches!(tree.len(), 40 | 64) && tree.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(tree)
}

/// The tree the run of `commit` whose record reads `record` was computed on: `UNPROVED` when the record is unread, is another run's, holds a run that proved no tree, or comes from an offloaded run that recorded no shipped run; else a recorded tree other than `commit_tree`, else `commit_tree`, the tree every step of a fresh checkout of the commit builds.
pub fn verdict_tree(record: Option<&str>, commit: &str, commit_tree: &str) -> String {
    let Some((first, rest)) = record.and_then(|text| text.split_once('\n')) else { return UNPROVED.to_owned() };
    let place = first.strip_prefix(RUN).and_then(|run| run.strip_prefix(commit)).and_then(|run| run.strip_prefix(' '));
    let Some(trees) = rest.lines().map(proved).collect::<Option<Vec<&str>>>() else { return UNPROVED.to_owned() };
    match place {
        Some(OFFLOADED) if !trees.is_empty() => trees.into_iter().find(|tree| *tree != commit_tree).unwrap_or(commit_tree).to_owned(),
        Some(HERE) => trees.into_iter().find(|tree| *tree != commit_tree).unwrap_or(commit_tree).to_owned(),
        _ => UNPROVED.to_owned(),
    }
}

/// The tree the red run of `commit` whose log is `log` was computed on, read from its record.
pub fn named(log: &Path, commit: &str, commit_tree: &str) -> String {
    verdict_tree(fs::read_to_string(record(log)).ok().as_deref(), commit, commit_tree)
}

/// Why a red verdict computed on `tree` judges nothing about a commit whose tree is `commit_tree`, none when it was computed on that tree.
pub fn misjudged(tree: &str, commit_tree: &str) -> Option<String> {
    (tree != commit_tree).then(|| format!("the red verdict was computed on tree {}, not the commit's tree {commit_tree}", if tree.is_empty() { "unnamed" } else { tree }))
}
