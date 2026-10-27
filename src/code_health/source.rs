use std::collections::HashSet;
use std::path::PathBuf;

pub(super) enum Source {
    Tree(HashSet<String>),
    Root(PathBuf),
}

impl Source {
    pub(super) fn tree(paths: Vec<String>) -> Self {
        Self::Tree(paths.into_iter().collect())
    }

    pub(super) fn root(root: PathBuf) -> Self {
        Self::Root(root)
    }

    pub(super) fn exists(&self, path: &str) -> bool {
        match self {
            Self::Tree(paths) => paths.contains(path),
            Self::Root(root) => root.join(path.trim_start_matches('/')).exists(),
        }
    }
}

pub(super) fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(at) => &path[..at],
        None => ".",
    }
}
