//! `north docs structure-index`: `structure.toml` from one file per table under `structure/`, so a
//! lane that adds a module, a crate or an allow entry edits its own file and never the generated
//! config.
use std::path::Path;

use super::docs::Index;
use super::registry::{files, read};

fn notes(source: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    let paths = files(source, "toml")?;
    let last = paths.last();
    for path in &paths {
        let text = read(path)?;
        if !text.trim_start().starts_with('[') {
            notes.push(format!("{}: the file does not start with a table header", path.display()));
        }
        if !text.ends_with('\n') {
            notes.push(format!("{}: the file does not end with a newline", path.display()));
        }
        if Some(path) != last && !text.ends_with("\n\n") {
            notes.push(format!("{}: the file does not end with the blank line that separates its table from the next", path.display()));
        }
    }
    Ok(notes)
}

pub const INDEX: Index = Index { command: "structure-index", source_dir: "structure", file: "structure.toml", render: north_registries::structure, notes };
