use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use regex::bytes::Regex;

use super::repo::git_out;

const BINARY_PROBE: usize = 8192;
const BUILD_OUTPUT: [&str; 4] = ["target", "node_modules", "dist", "build"];

pub(super) struct Scope {
    pub(super) max_file_size: Option<u64>,
    pub(super) skip_build_output: bool,
    pub(super) limit: Option<Duration>,
}

pub(super) fn mentioning(roots: &[PathBuf], needle: &Regex, scope: &Scope) -> Result<Vec<PathBuf>, String> {
    let started = Instant::now();
    let mut hits = Vec::new();
    for root in roots {
        for relative in candidates(root) {
            if scope.limit.is_some_and(|limit| started.elapsed() > limit) {
                return Err("search timed out".into());
            }
            if skipped(&relative, scope) {
                continue;
            }
            let path = root.join(&relative);
            if mentions(&path, needle, scope)? {
                hits.push(path);
            }
        }
    }
    Ok(hits)
}

fn candidates(root: &Path) -> Vec<PathBuf> {
    match git_out(root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]) {
        Some(listing) => listing.split('\0').filter(|name| !name.is_empty()).map(PathBuf::from).collect(),
        None => walk(root, root),
    }
}

fn walk(base: &Path, directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => found.extend(walk(base, &path)),
            Ok(kind) if kind.is_file() => found.extend(path.strip_prefix(base).map(Path::to_path_buf)),
            _ => {}
        }
    }
    found
}

fn skipped(relative: &Path, scope: &Scope) -> bool {
    let names = relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>();
    let Some((leaf, directories)) = names.split_last() else {
        return true;
    };
    names.iter().any(|name| name == ".git")
        || (scope.skip_build_output
            && (leaf == "result" || directories.iter().any(|name| name == "result" || BUILD_OUTPUT.contains(&name.as_ref()))))
}

fn mentions(path: &Path, needle: &Regex, scope: &Scope) -> Result<bool, String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(false);
    };
    if !metadata.is_file() || scope.max_file_size.is_some_and(|limit| metadata.len() > limit) {
        return Ok(false);
    }
    let mut content = Vec::new();
    fs::File::open(path)
        .and_then(|mut file| file.read_to_end(&mut content))
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if content.iter().take(BINARY_PROBE).any(|byte| *byte == 0) {
        return Ok(false);
    }
    Ok(needle.is_match(&content))
}
