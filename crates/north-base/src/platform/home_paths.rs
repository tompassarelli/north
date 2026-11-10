use std::path::{Path, PathBuf};

/// Every absolute path the prompt names under `home`, with a leading `~/` expanded: a file the
/// launching host can read and a session placed on another machine cannot.
pub fn home_paths(prompt: &str, home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for word in prompt.split_whitespace() {
        let word = word.trim_matches(|c: char| "\"'`(<[{)]}>,;:.".contains(c));
        let path = word.strip_prefix("~/").map_or_else(|| PathBuf::from(word), |rest| home.join(rest));
        if path != home && path.starts_with(home) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}
