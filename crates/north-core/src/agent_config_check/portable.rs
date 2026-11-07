use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::gate::frontmatter;

const EXCLUDED_TEXT: [&str; 5] = ["PROVENANCE.md", "NOTICE", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE"];
const FORBIDDEN_DIRS: [&str; 4] = ["adapters", "hooks", "providers", "secrets"];
const PROVIDER_INTERFACE_METADATA: &str = "agents/openai.yaml";
const LARGEST_SCANNED_FILE: u64 = 1_000_000;

static FORBIDDEN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/home/|~/|mcp__|\b(?:North|Firn|NixOS|Beagle)\b").expect("forbidden text pattern"));
static PORTABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^portable:[ \t]*true[ \t]*$").expect("portable pattern"));

pub(super) fn declared(skill_text: &str) -> bool {
    frontmatter(skill_text).is_some_and(|(front, _)| PORTABLE.is_match(&front))
}

pub(super) fn boundary(tree: &Path, skill_dir: &Path, unit: &str) -> Vec<String> {
    let files = match walk(skill_dir) {
        Ok(files) => files,
        Err(error) => return vec![format!("{unit}: {}: {error}", skill_dir.display())],
    };
    let mut errors = Vec::new();
    for path in files {
        let shown = path.strip_prefix(tree).unwrap_or(&path).display().to_string();
        let relative = path.strip_prefix(skill_dir).unwrap_or(&path).to_string_lossy().into_owned();
        let segments: Vec<&str> = relative.split('/').collect();
        if segments[..segments.len() - 1].iter().any(|segment| FORBIDDEN_DIRS.contains(segment)) {
            errors.push(format!("{unit}: forbidden source boundary directory in {shown}"));
        }
        if relative == PROVIDER_INTERFACE_METADATA || relative.ends_with(&format!("/{PROVIDER_INTERFACE_METADATA}")) {
            errors.push(format!("{unit}: provider interface metadata in {shown}"));
        }
        let size = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                errors.push(format!("{unit}: {shown}: {error}"));
                continue;
            }
        };
        if EXCLUDED_TEXT.contains(&relative.as_str()) || size > LARGEST_SCANNED_FILE {
            continue;
        }
        match fs::read(&path) {
            Ok(bytes) if FORBIDDEN_TEXT.is_match(&String::from_utf8_lossy(&bytes)) => {
                errors.push(format!("{unit}: non-portable source marker in {shown}"));
            }
            Ok(_) => {}
            Err(error) => errors.push(format!("{unit}: {shown}: {error}")),
        }
    }
    errors
}

fn walk(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    let mut files = Vec::new();
    for entry in entries {
        if matches!(entry.file_name().to_str(), Some(".git" | "node_modules")) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            files.extend(walk(&entry.path())?);
        } else {
            files.push(entry.path());
        }
    }
    Ok(files)
}
