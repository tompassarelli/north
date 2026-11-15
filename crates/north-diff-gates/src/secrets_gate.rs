//! The landing side of the secrets/ rule (north#1626): every file under secrets/ that a lane adds or changes must be sealed, as `north_shared_config::secrets_seal` decides.
use std::path::Path;

use north_base::platform::git as platform_git;
use north_shared_config::secrets_seal::{DIRECTORY, sealed};

use crate::language_gate::EMPTY_TREE;

type Result<T> = std::result::Result<T, String>;

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    platform_git::stdout(repo, args, |output| format!("git {} failed: {}", args[0], String::from_utf8_lossy(&output.stderr).trim()))
}

/// Each file under secrets/ that `base` and `head` add or change and that is not sealed, as `PATH: not sops-encrypted or a public key`.
pub fn violations(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let base = if base.is_empty() { EMPTY_TREE } else { base };
    let listed = git(repo, &["diff", "--name-only", "-z", "--no-renames", "--diff-filter=AM", base, head, "--", DIRECTORY])?;
    let mut found = Vec::new();
    for path in String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()) {
        let content = git(repo, &["show", &format!("{head}:{path}")])?;
        if !sealed(path, &content) {
            found.push(format!("{path}: not sops-encrypted and not an OpenSSH ed25519 public key"));
        }
    }
    Ok(found)
}

pub fn check(repo: &Path, base: &str, head: &str) -> bool {
    match violations(repo, base, head) {
        Ok(found) if found.is_empty() => {
            println!("secrets-gate: passed");
            true
        }
        Ok(found) => {
            for line in found {
                println!("secrets-gate: {line}");
            }
            false
        }
        Err(error) => {
            eprintln!("secrets-gate: {error}");
            false
        }
    }
}
