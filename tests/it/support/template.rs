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
        self.tree.write(destination);
    }
}
