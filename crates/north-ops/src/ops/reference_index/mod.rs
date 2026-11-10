use std::fs;
use std::path::{Path, PathBuf};

use super::docs::Index;

const HEADER_FILE: &str = "_header.txt";
const FOOTER_FILE: &str = "_footer.txt";

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn block(text: &str) -> String {
    format!("{}\n", text.trim_end_matches('\n'))
}

fn sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|error| format!("{}: {error}", dir.display())))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

fn hidden(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_none_or(|name| name.starts_with('_') || name.starts_with('.'))
}

fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(sorted(dir)?.into_iter().filter(|path| !hidden(path) && path.extension().is_some_and(|extension| extension == "txt")).collect())
}

/// The reference is one file per command, named `NNNN-command.txt` so name order is the command
/// order, holding that command's reference line verbatim.
pub fn render(source: &Path) -> Result<String, String> {
    let mut out = block(&read(&source.join(HEADER_FILE))?);
    for path in entries(source)? {
        out.push_str(&block(&read(&path)?));
    }
    out.push_str(&block(&read(&source.join(FOOTER_FILE))?));
    Ok(out)
}

fn notes(source: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for path in entries(source)? {
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

pub const INDEX: Index = Index { command: "reference-index", source_dir: "src/cli/reference", file: "src/cli/reference.txt", render, notes };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_is_the_header_and_footer_around_each_command_file_in_name_order() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path();
        std::fs::write(source.join("_header.txt"), "Head\n").unwrap();
        std::fs::write(source.join("_footer.txt"), "\nFoot\n").unwrap();
        std::fs::write(source.join("0002-second.txt"), "  second --flag  Later\n").unwrap();
        std::fs::write(source.join("0001-first.txt"), "  first         Earlier\n").unwrap();
        std::fs::write(source.join("_ignored.txt"), "  ignored\n").unwrap();

        assert_eq!(render(source).unwrap(), "Head\n  first         Earlier\n  second --flag  Later\n\nFoot\n");
    }

    #[test]
    fn a_file_whose_line_is_not_indented_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path();
        std::fs::write(source.join("_header.txt"), "Head\n").unwrap();
        std::fs::write(source.join("_footer.txt"), "Foot\n").unwrap();
        std::fs::write(source.join("0001-first.txt"), "first\n").unwrap();

        assert_eq!(notes(source).unwrap(), vec![format!("{}: the first line is not an indented reference line", source.join("0001-first.txt").display())]);
    }
}
