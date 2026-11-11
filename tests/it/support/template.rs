use std::fs;
use std::path::{Path, PathBuf};

use super::tree::Tree;

pub struct Template {
    built_at: PathBuf,
    tree: Tree,
}

impl Template {
    pub fn build(fill: impl FnOnce(&Path)) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        fill(scratch.path());
        Self { built_at: scratch.path().to_owned(), tree: Tree::read(scratch.path()) }
    }

    pub fn built_at(&self) -> &Path {
        &self.built_at
    }

    pub fn copy_into(&self, destination: &Path) {
        let used = fs::read_dir(destination).is_ok_and(|mut entries| entries.next().is_some());
        assert!(!used, "{} already holds files, and a template fills only an empty root: a process from an earlier step may still have one of them open", destination.display());
        self.tree.write(destination);
    }
}
