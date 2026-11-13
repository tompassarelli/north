pub const UNTRACKED: &[(&str, &str)] = &[("docs/features.md", "docs/features/")];

pub struct Registry {
    pub path: String,
    pub sources: Vec<String>,
}

pub fn registries() -> Vec<Registry> {
    UNTRACKED.iter().map(|(path, source)| Registry { path: (*path).to_owned(), sources: vec![(*source).to_owned()] }).collect()
}

/// A registry the repository does not track is written by `north docs`, so a lane that commits it
/// is refused. A tracked generated file (a `regenerate` rule of the merge-set config) is never
/// refused: the lander rewrites it from its sources, whatever the lane committed.
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
