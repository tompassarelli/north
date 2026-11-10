use std::collections::{HashMap, HashSet};
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

struct Candidate {
    relative: PathBuf,
    blob: Option<String>,
}

pub(super) fn mentioning(roots: &[PathBuf], needle: &Regex, scope: &Scope) -> Result<Vec<PathBuf>, String> {
    let started = Instant::now();
    let mut hits = Vec::new();
    let mut verdicts: HashMap<String, bool> = HashMap::new();
    for root in roots {
        for candidate in candidates(root) {
            if scope.limit.is_some_and(|limit| started.elapsed() > limit) {
                return Err("search timed out".into());
            }
            if skipped(&candidate.relative, scope) {
                continue;
            }
            let path = root.join(&candidate.relative);
            let mentioned = match &candidate.blob {
                Some(blob) => match verdicts.get(blob) {
                    Some(known) => *known,
                    None => {
                        let found = mentions(&path, needle, scope)?;
                        verdicts.insert(blob.clone(), found);
                        found
                    }
                },
                None => mentions(&path, needle, scope)?,
            };
            if mentioned {
                hits.push(path);
            }
        }
    }
    Ok(hits)
}

fn candidates(root: &Path) -> Vec<Candidate> {
    match git_out(root, &["ls-files", "-z", "-t", "-s", "-m", "-o", "--exclude-standard"]) {
        Some(listing) => tracked_and_untracked(&listing),
        None => walk(root, root).into_iter().map(|relative| Candidate { relative, blob: None }).collect(),
    }
}

fn tracked_and_untracked(listing: &str) -> Vec<Candidate> {
    let entries: Vec<(&str, &str)> = listing.split('\0').filter_map(|entry| entry.split_once(' ')).collect();
    let changed: HashSet<&str> = entries.iter().filter(|(tag, _)| *tag == "C").filter_map(|(_, rest)| rest.split_once('\t').map(|(_, path)| path)).collect();
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (tag, rest) in entries {
        let (blob, path) = match (tag, rest.split_once('\t')) {
            ("H", Some((stage, path))) => (regular_blob(stage), path),
            ("?", None) => (None, rest),
            _ => continue,
        };
        if !seen.insert(path.to_owned()) {
            continue;
        }
        let blob = blob.filter(|_| !changed.contains(path));
        found.push(Candidate { relative: PathBuf::from(path), blob });
    }
    found
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
    let unreadable = |error: std::io::Error| format!("cannot read {}: {error}", path.display());
    let mut file = fs::File::open(path).map_err(unreadable)?;
    let mut content = Vec::new();
    (&mut file).take(BINARY_PROBE as u64).read_to_end(&mut content).map_err(unreadable)?;
    if content.contains(&0) {
        return Ok(false);
    }
    file.read_to_end(&mut content).map_err(unreadable)?;
    Ok(needle.is_match(&content))
}

fn regular_blob(stage: &str) -> Option<String> {
    let mut fields = stage.split(' ');
    let mode = fields.next()?;
    let blob = fields.next()?;
    mode.starts_with("100").then(|| blob.to_owned())
}
