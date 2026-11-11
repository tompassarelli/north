use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

const ROOT: &str = "\u{1}checkout\u{1}";

fn memo_dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("north/cargo-metadata")
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn manifests_key(root: &Path) -> Option<String> {
    let clean = git(root, &["status", "--porcelain", "--untracked-files=all"])?.is_empty();
    let manifests = git(root, &["ls-files", "-s", "--", "Cargo.toml", "*/Cargo.toml", "*config.toml", "rust-toolchain.toml", "rust-toolchain"])?;
    let names = git(root, &["ls-files"])?;
    let environment = format!("{:?} {:?}", std::env::var_os("CARGO_TARGET_DIR"), std::env::var_os("CARGO_BUILD_TARGET_DIR"));
    let home = PathBuf::from(std::env::var_os("CARGO_HOME").unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cargo").into_os_string()));
    let configs: String = root.ancestors().skip(1).map(|dir| dir.join(".cargo/config.toml")).chain([home.join("config.toml")]).map(|path| format!("{}\0{}\n", path.display(), fs::read_to_string(&path).unwrap_or_default())).collect();
    clean.then(|| Sha256::digest(format!("{manifests}\0{names}\0{environment}\0{configs}").as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn no_deps(root: &Path) -> Result<String, String> {
    no_deps_in(root, &memo_dir())
}

fn quoted(root: &str) -> [(String, String); 3] {
    [(format!("\"{root}\""), format!("\"{ROOT}\"")), (format!("\"{root}/"), format!("\"{ROOT}/")), (format!("file://{root}/"), format!("file://{ROOT}/"))]
}

fn swap(text: &str, pairs: &[(String, String); 3], to_placeholder: bool) -> String {
    pairs.iter().fold(text.to_owned(), |text, (path, placeholder)| if to_placeholder { text.replace(path, placeholder) } else { text.replace(placeholder, path) })
}

fn no_deps_in(root: &Path, memo_dir: &Path) -> Result<String, String> {
    let root_text = fs::canonicalize(root).map_or_else(|_| root.to_string_lossy().into_owned(), |path| path.to_string_lossy().into_owned());
    let plain = !root_text.is_empty() && !root_text.contains(['"', '\\']);
    let pairs = quoted(&root_text);
    let key = manifests_key(root).filter(|_| plain);
    let memo = key.as_ref().map(|key| memo_dir.join(key));
    if let Some(text) = memo.as_ref().and_then(|path| fs::read_to_string(path).ok()) {
        return Ok(swap(&text, &pairs, false));
    }
    let output = Command::new("cargo").args(["metadata", "--no-deps", "--format-version", "1", "--offline"]).current_dir(root).stdin(Stdio::null()).output().map_err(|error| format!("cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata exited {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if let Some(path) = memo
        && fs::create_dir_all(memo_dir).is_ok()
        && let Ok(pending) = tempfile::NamedTempFile::new_in(memo_dir)
        && fs::write(pending.path(), swap(&text, &pairs, true)).is_ok()
    {
        let _ = pending.persist(path);
    }
    Ok(text)
}

#[cfg(test)]
#[path = "cargo_metadata_tests.rs"]
mod tests;
