use super::functions::SourceText;
use super::imports::imports_of;
use super::rust_modules::Crate;
use super::snapshot::Snapshot;
use super::source::Source;
use std::path::Path;

pub(super) struct Resolver {
    source: Source,
    crates: Vec<Crate>,
}

fn is_manifest(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

impl Resolver {
    #[cfg(test)]
    pub(super) fn new(source: Source, manifests: &[(&str, &str)]) -> Self {
        Self { source, crates: Crate::from_manifests(manifests.iter().copied()) }
    }

    pub(super) fn of_snapshot(snapshot: &Snapshot) -> Self {
        let manifests: Vec<(String, String)> = snapshot.paths().iter().filter(|path| is_manifest(path)).map(|path| (path.clone(), snapshot.text(path).map(|text| text.to_string()).unwrap_or_default())).collect();
        Self { source: Source::tree(snapshot.paths().to_vec()), crates: Crate::from_manifests(manifests.iter().map(|(path, text)| (path.as_str(), text.as_str()))) }
    }

    pub(super) fn of_worktree(repo: &Path, head: &Snapshot) -> Self {
        let manifests: Vec<(String, String)> = head.paths().iter().filter(|path| is_manifest(path)).map(|path| (path.clone(), std::fs::read_to_string(repo.join(path)).unwrap_or_default())).collect();
        Self { source: Source::root(repo.to_path_buf()), crates: Crate::from_manifests(manifests.iter().map(|(path, text)| (path.as_str(), text.as_str()))) }
    }

    pub(super) fn imports(&self, path: &str, text: &str) -> Vec<String> {
        imports_of(path, text, &self.source, &self.crates)
    }

    pub(super) fn imports_all(&self, files: &[SourceText]) -> Vec<Vec<String>> {
        files.iter().map(|file| self.imports(&file.path, &file.text)).collect()
    }
}
