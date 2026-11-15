use std::path::{Component, Path, PathBuf};

/// Every absolute path the prompt names under `home`, with a leading `~/` expanded, that the launching host holds and only it can: a file a session placed on another machine cannot read. A path this host does not hold is no such file. A path under a repository's folder in `~/code/REPO` (its checkouts, mirror and worktrees) or under the cargo registry is no such file, since every fleet host keeps those.
pub fn home_paths(prompt: &str, home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for word in prompt.split_whitespace() {
        let word = word.trim_matches(|c: char| "\"'`(<[{)]}>,;:.".contains(c));
        let path = expand(word, home);
        if path != home && path.starts_with(home) && !fleet_wide(&path, home) && !paths.contains(&path) && path.exists() {
            paths.push(path);
        }
    }
    paths
}

/// The paths `home_paths` finds in `prompt` other than those inside `started_in`, the directory a session starts in, which the machine it is placed on has.
pub fn outside(prompt: &str, home: &Path, started_in: &Path) -> Vec<PathBuf> {
    let exempt = started_in != home && started_in.starts_with(home);
    home_paths(prompt, home).into_iter().filter(|path| !(exempt && path.starts_with(started_in))).collect()
}

pub fn expand(word: &str, home: &Path) -> PathBuf {
    if let Some(rest) = word.strip_prefix("~/").or_else(|| word.strip_prefix("$HOME/")) {
        return home.join(rest);
    }
    word.strip_prefix("$XDG_STATE_HOME/").map_or_else(|| PathBuf::from(word), |rest| home.join(".local/state").join(rest))
}

fn fleet_wide(path: &Path, home: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(home) else { return false };
    let Some(parts) = relative.components().map(|part| match part { Component::Normal(name) => name.to_str(), _ => None }).collect::<Option<Vec<&str>>>() else { return false };
    matches!(parts.as_slice(), ["code", _, ..] | [".cargo", "registry", ..])
}
