use std::path::Path;

use super::merge_set::{self, Kind};

pub const UNTRACKED: &[(&str, &str)] = &[("docs/features.md", "docs/features/")];

pub struct Registry {
    pub path: String,
    pub sources: Vec<String>,
}

pub fn registries(config: &Path) -> Result<Vec<Registry>, String> {
    let mut registries: Vec<Registry> = UNTRACKED.iter().map(|(path, source)| Registry { path: (*path).to_owned(), sources: vec![(*source).to_owned()] }).collect();
    for rule in merge_set::read_rules(config)? {
        if let Kind::Regenerate { sources, .. } = rule.kind {
            registries.push(Registry { path: rule.path, sources });
        }
    }
    Ok(registries)
}

/// A generated registry is written at land time from its per-unit sources. A lane that commits
/// the generated file by hand, or regenerates it itself, is refused so the file never enters a
/// worker's diff and the lander's refresh owns the only copy.
pub fn refusal(registries: &[Registry], changed: &[String]) -> Result<(), String> {
    for Registry { path, sources } in registries {
        if !changed.contains(path) {
            continue;
        }
        let source = changed.iter().find(|file| file != &path && sources.iter().any(|prefix| file.starts_with(prefix.as_str())));
        let edit = source.map_or_else(|| format!("a file under {}", sources.join(", ")), Clone::clone);
        return Err(format!(
            "{path} is generated from {}; edit {edit} instead, remove {path} from the commit, and let the lander write it at land time; NOT queued.",
            sources.join(", "),
        ));
    }
    Ok(())
}
