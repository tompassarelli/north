//! The knowledge the equipped T3 Code arm is given: the owner's CLAUDE.md and skills directory, copied beside a settings file that holds only the safety rules
//! as hooks (secrets, no force-push, no deleting home, the language and system-policy guards). North's dispatch, routing and fleet guards are left out on purpose.
//! One sha256 over the copied tree names the configuration in the run row.
use std::fs;
use std::path::{Path, PathBuf};

use north_base::error::NorthError;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const COPIED: [&str; 2] = ["CLAUDE.md", "skills"];

fn hook(north: &Path, verb: &str, matcher: &str) -> Value {
    json!({"matcher": matcher, "hooks": [{"type": "command", "command": format!("{} hook {verb}", north.display()), "timeout": 60}]})
}

fn settings(north: &Path) -> Value {
    json!({
        "permissions": {"defaultMode": "bypassPermissions"},
        "skipDangerousModePermissionPrompt": true,
        "hooks": {"PreToolUse": [
            hook(north, "language-guard", "Write|Edit|MultiEdit|NotebookEdit|Bash"),
            hook(north, "firn-system-policy", "Edit|Write|MultiEdit"),
            hook(north, "safety", "Bash"),
        ]}
    })
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), NorthError> {
    let source = fs::canonicalize(from).map_err(NorthError::Io)?;
    if source.is_file() {
        return fs::copy(&source, to).map(drop).map_err(NorthError::Io);
    }
    fs::create_dir_all(to).map_err(NorthError::Io)?;
    for entry in fs::read_dir(&source).map_err(NorthError::Io)?.flatten() {
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

fn files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// The sha256 of every file under `dir`, each taken by its relative path and its bytes in path order.
pub(crate) fn pin(dir: &Path) -> Result<String, NorthError> {
    let mut found = Vec::new();
    files(dir, &mut found);
    found.sort();
    let mut digest = Sha256::new();
    for path in found {
        digest.update(path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(fs::read(&path).map_err(NorthError::Io)?);
        digest.update([0]);
    }
    Ok(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Builds the arm's configuration directory at `dest` from `owner`'s CLAUDE.md and skills and returns its pin; its hooks run the very `north` that built it, so the verbs they name always exist.
pub(crate) fn assemble(owner: &Path, dest: &Path) -> Result<String, NorthError> {
    fs::create_dir_all(dest).map_err(NorthError::Io)?;
    let north = std::env::current_exe().map_err(NorthError::Io)?;
    for name in COPIED {
        copy_tree(&owner.join(name), &dest.join(name))?;
    }
    fs::write(dest.join("settings.json"), serde_json::to_string_pretty(&settings(&north)).map_err(NorthError::Json)?).map_err(NorthError::Io)?;
    pin(dest)
}
