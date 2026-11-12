//! The pass of a finish call's check, recorded by the tree the lane commits, so the check before the ticket that runs the same command on that tree reuses it instead of running it a second time in the same hand-in (north#1230).

use std::path::Path;

use north_base::foundation::clock::Timestamp;
use north_base::verdict_cache::{Pass, Store};

use super::git;

pub(super) struct Tested {
    tree: String,
    key: String,
}

impl Tested {
    /// The pass `check` earns on the tree of `head`, when the check is plain words and the worktree differs from HEAD only in the named `paths`, so the check runs on exactly that tree.
    pub(super) fn of(root: &Path, check: &str, head: &str, paths: &[String]) -> Option<Self> {
        let canonical = north_tree_verdict::plain(check)?;
        let unstaged = git(root, &["ls-files", "--modified", "--deleted", "--others", "--exclude-standard"]).ok()?;
        let staged = git(root, &["diff", "--cached", "--name-only", "HEAD"]).ok()?;
        if !unstaged.lines().chain(staged.lines()).all(|path| paths.iter().any(|named| Path::new(named) == root.join(path))) {
            return None;
        }
        let tree = north_tree_verdict::tree(root, head)?;
        Some(Self { key: north_tree_verdict::key(&tree, &canonical), tree })
    }

    /// Records the pass once the lane is committed and its worktree holds exactly the tree the check ran on.
    pub(super) fn settle(self, root: &Path) {
        if north_tree_verdict::clean_tree(root).as_deref() == Some(self.tree.as_str()) {
            let _ = Store::here().record(&self.key, Some(&Pass { at: Timestamp::now(), sha: self.tree }));
        }
    }
}
