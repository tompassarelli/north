//! The tree an after-landing verdict was computed on, named in the sheriff's request so a red verdict on any other tree reverts nothing.
use north_base::foundation::offload::PROVED_TREE;

const RUNNING: &str = "offload: running ";
pub const UNPROVED: &str = "unproved";

fn ran_a_commit(line: &str) -> bool {
    line.find(RUNNING).is_some_and(|at| line[at + RUNNING.len()..].split_whitespace().next().is_some_and(|word| word.len() == 40 && word.bytes().all(|byte| byte.is_ascii_hexdigit())))
}

/// The tree the run whose output is `log` was computed on: a tree some offloaded step proved other than `commit_tree`, `UNPROVED` when an offloaded step ran without proving its tree, else `commit_tree`, the tree every step of a fresh checkout of the commit builds.
pub fn verdict_tree(log: &str, commit_tree: &str) -> String {
    let proved: Vec<&str> = log.lines().filter_map(|line| line.find(PROVED_TREE).and_then(|at| line[at + PROVED_TREE.len()..].split_whitespace().next())).collect();
    if log.lines().filter(|line| ran_a_commit(line)).count() > proved.len() {
        return UNPROVED.to_owned();
    }
    proved.into_iter().find(|tree| *tree != commit_tree).unwrap_or(commit_tree).to_owned()
}

/// Why a red verdict computed on `tree` judges nothing about a commit whose tree is `commit_tree`, none when it was computed on that tree.
pub fn misjudged(tree: &str, commit_tree: &str) -> Option<String> {
    (tree != commit_tree).then(|| format!("the red verdict was computed on tree {}, not the commit's tree {commit_tree}", if tree.is_empty() { "unnamed" } else { tree }))
}
