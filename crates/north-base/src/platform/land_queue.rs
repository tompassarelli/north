use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub fn key(origin: &str) -> String {
    let origin = origin.trim_end_matches('/');
    let origin = if origin.ends_with(".git") { origin.to_owned() } else { format!("{origin}.git") };
    Sha256::digest(origin.as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

pub fn directory(state_home: &Path, origin: &str) -> PathBuf {
    state_home.join("north/land-queue").join(key(origin))
}
