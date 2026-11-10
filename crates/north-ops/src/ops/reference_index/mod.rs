//! `north docs reference-index`: `src/cli/reference.txt` from one file per command under
//! `src/cli/reference/`, so a lane that adds or renames a command edits its own file and never the
//! generated reference.
use std::path::Path;

use super::docs::Index;
use super::registry::{files, read};

fn notes(source: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for path in files(source, "txt")? {
        let text = read(&path)?;
        let first = text.lines().next().unwrap_or_default();
        if !first.starts_with("  ") || first.trim().is_empty() {
            notes.push(format!("{}: the first line is not an indented reference line", path.display()));
        }
        if !text.ends_with('\n') {
            notes.push(format!("{}: the file does not end with a newline", path.display()));
        }
    }
    Ok(notes)
}

pub const INDEX: Index = Index { command: "reference-index", source_dir: "src/cli/reference", file: "src/cli/reference.txt", render: north_registries::reference, notes };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_whose_line_is_not_indented_is_reported() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("0001-first.txt"), "first\n").unwrap();
        assert_eq!(notes(root.path()).unwrap(), vec![format!("{}: the first line is not an indented reference line", root.path().join("0001-first.txt").display())]);
    }
}
