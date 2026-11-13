use crate::platform::git as platform_git;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub(crate) fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let stdout = platform_git::stdout(root, args, |output| {
        format!(
            "git {} failed (exit {}); check repository and revisions",
            args[0],
            output.status.code().unwrap_or(-1)
        )
    })?;
    String::from_utf8(stdout).map_err(|e| e.to_string())
}

pub(crate) struct Snapshot {
    pub(super) manifests: BTreeMap<String, String>,
    pub(crate) locks: BTreeMap<String, String>,
    pub(super) settings: Option<String>,
}

const MANIFEST_NAMES: [&str; 3] = ["package.json", "Cargo.toml", "pyproject.toml"];
const LOCK_NAMES: [&str; 3] = ["bun.lock", "Cargo.lock", "flake.lock"];
const SETTINGS: &str = ".safe-push";

pub(crate) fn read_snapshot(root: &Path, revision: &str) -> Result<Snapshot, String> {
    let listing = git(root, &["ls-tree", "-r", "-z", revision])?;
    let mut wanted: Vec<(&str, &str, bool)> = Vec::new();
    for entry in listing.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let Some(oid) = meta.split(' ').nth(2) else { continue };
        let filename = path.rsplit('/').next().unwrap_or("");
        let manifest = MANIFEST_NAMES.contains(&filename);
        if !path.starts_with("repos/") && (manifest || LOCK_NAMES.contains(&filename) || path == SETTINGS) {
            wanted.push((path, oid, manifest));
        }
    }
    let request: String = wanted.iter().map(|(_, oid, _)| format!("{oid}\n")).collect();
    let root_text = root.to_string_lossy();
    let output = crate::platform::process::run_with_input_within(
        "git",
        &["-C", root_text.as_ref(), "cat-file", "--batch"],
        Some(request.as_bytes()),
        Duration::from_secs(60),
    )
    .filter(|output| output.status.success())
    .ok_or("git cat-file failed; check repository and revisions")?;
    let bytes = output.stdout;
    let mut at = 0;
    let mut snapshot = Snapshot { manifests: BTreeMap::new(), locks: BTreeMap::new(), settings: None };
    for (path, _, manifest) in wanted {
        let header_end = at + bytes[at..].iter().position(|&byte| byte == b'\n').ok_or("git cat-file output ended early")?;
        let header = String::from_utf8_lossy(&bytes[at..header_end]);
        let size: usize = header.rsplit(' ').next().and_then(|size| size.parse().ok()).ok_or_else(|| format!("git cat-file: {path}: {header}"))?;
        let body = bytes.get(header_end + 1..header_end + 1 + size).ok_or("git cat-file output ended early")?;
        let text = String::from_utf8(body.to_vec()).map_err(|e| e.to_string())?;
        at = header_end + 1 + size + 1;
        if manifest { snapshot.manifests.insert(path.to_owned(), text); } else if path == SETTINGS { snapshot.settings = Some(text); } else { snapshot.locks.insert(path.to_owned(), text); }
    }
    Ok(snapshot)
}
