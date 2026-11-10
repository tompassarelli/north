use std::path::{Component, Path, PathBuf};

/// Every absolute path the prompt names under `home`, with a leading `~/` expanded, that only the launching host holds: a file a session placed on another machine cannot read. A path inside a repository's primary checkout, `~/code/REPO/main`, is no such file, since every fleet host keeps that checkout.
pub fn home_paths(prompt: &str, home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for word in prompt.split_whitespace() {
        let word = word.trim_matches(|c: char| "\"'`(<[{)]}>,;:.".contains(c));
        let path = word.strip_prefix("~/").map_or_else(|| PathBuf::from(word), |rest| home.join(rest));
        if path != home && path.starts_with(home) && !in_primary_checkout(&path, home) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn in_primary_checkout(path: &Path, home: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(home) else { return false };
    let mut parts = relative.components();
    matches!((parts.next(), parts.next(), parts.next()), (Some(Component::Normal(code)), Some(Component::Normal(_)), Some(Component::Normal(main))) if code == "code" && main == "main")
        && parts.all(|part| matches!(part, Component::Normal(_)))
}
