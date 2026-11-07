use super::config::at_rev;
use super::functions::SourceText;
use super::gate::Violation;
use super::report::{hand_of, inventory, texts};
use super::snapshot::Snapshot;
use north_implicit_state::{self as implicit, Config, Source};
use std::path::Path;

pub(super) const RULE: &str = "implicit-state";

fn sources<'a>(files: impl IntoIterator<Item = &'a SourceText>) -> Vec<Source<'a>> {
    files.into_iter().map(|file| Source { path: &file.path, text: &file.text }).collect()
}

fn scanned(path: &str) -> bool {
    [".rs", ".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".jsx"].iter().any(|extension| path.ends_with(extension))
}

/// The rule's `[implicit_state]` table and the hand-written sources at `snapshot`, so a presence check is
/// judged by what the whole repository writes.
fn repository(snapshot: &mut Snapshot, override_path: Option<&Path>) -> Result<(Config, Vec<SourceText>), String> {
    let structure = match override_path {
        Some(path) => std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?,
        None => snapshot.text("structure.toml").map(|text| text.to_string()).unwrap_or_default(),
    };
    let hand: Vec<String> = hand_of(&inventory(snapshot, &at_rev(snapshot, override_path)?)).into_iter().filter(|path| scanned(path)).collect();
    Ok((Config::parse(&structure)?, texts(snapshot, &hand)?))
}

/// The implicit state `head` adds over `base`, each as a violation naming its shape, the fix and the exception.
pub(super) fn debt(snapshot: &mut Snapshot, override_path: Option<&Path>, head: &[SourceText], base: &[SourceText]) -> Result<Vec<Violation>, String> {
    let (config, context) = repository(snapshot, override_path)?;
    let found = implicit::added(&config, &sources(head.iter().filter(|file| scanned(&file.path))), &sources(base), &sources(&context))?;
    Ok(found.iter().map(|finding| Violation { line: Some(finding.line), ..Violation::new(RULE, &finding.path, finding.describe(&config)) }).collect())
}

/// `north code-health implicit-state`: every finding in the repository at `rev`, counted by shape.
pub(super) fn listing(repo: &Path, rev: &str, override_path: Option<&Path>) -> Result<String, String> {
    let (config, files) = repository(&mut Snapshot::new(repo, rev)?, override_path)?;
    implicit::listing(&config, &sources(&files))
}
