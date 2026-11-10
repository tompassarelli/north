//! `north docs verbs-index`: `src/cli/verbs.rs` from one file per verb under `src/cli/verbs/`, so a
//! lane that adds a verb edits its own file and never the generated table.
use std::path::Path;

use super::docs::Index;
use super::registry::{files, read};

fn notes(source: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for path in files(source, "txt")? {
        let text = read(&path)?;
        if !text.trim_start().starts_with("(\"") {
            notes.push(format!("{}: the first line is not a verb table line", path.display()));
        }
        if !text.ends_with('\n') {
            notes.push(format!("{}: the file does not end with a newline", path.display()));
        }
    }
    Ok(notes)
}

pub const INDEX: Index = Index { command: "verbs-index", source_dir: "src/cli/verbs", file: "src/cli/verbs.rs", render: north_registries::verbs, notes };
