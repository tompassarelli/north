use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const GITHUB_SSH: [&str; 2] = ["git@github.com:", "ssh://git@github.com/"];

/// The queue a repository's landings wait in, keyed by its GitHub identity rather than the spelling of its origin URL, so hosts that clone it over ssh and over https share one queue.
pub fn key(origin: &str) -> String {
    Sha256::digest(canonical(origin).as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

fn canonical(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    let origin = origin.strip_suffix(".git").unwrap_or(origin);
    match GITHUB_SSH.iter().find_map(|prefix| origin.strip_prefix(prefix)) {
        Some(path) => format!("https://github.com/{path}.git"),
        None => format!("{origin}.git"),
    }
}

pub fn directory(state_home: &Path, origin: &str) -> PathBuf {
    state_home.join("north/land-queue").join(key(origin))
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn every_spelling_of_one_github_repository_names_one_queue_and_the_https_key_is_unchanged() {
        let https = key("https://github.com/tompassarelli/wisp");
        for spelling in ["https://github.com/tompassarelli/wisp.git", "https://github.com/tompassarelli/wisp/", "git@github.com:tompassarelli/wisp.git", "git@github.com:tompassarelli/wisp", "ssh://git@github.com/tompassarelli/wisp.git"] {
            assert_eq!(key(spelling), https, "{spelling}");
        }
        assert_eq!(https, "560e3f31dbaa2415");
        assert_eq!(key("https://github.com/tompassarelli/south"), "d9dc45544422e167");
        assert_ne!(key("https://github.com/tompassarelli/makgora"), https);
        assert_ne!(key("https://gitlab.com/tompassarelli/wisp"), https);
    }
}
