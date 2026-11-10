//! The generated registries: each is one file built from a tree of per-unit source files, so a lane
//! that adds a command, a verb or a module edits one small file and never the generated one. The
//! build script and `north docs` share these renderers: the build writes a registry before it is
//! compiled, and the lander writes it after it picks a lane that changed its sources.
use std::fs;
use std::path::{Path, PathBuf};

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn block(text: &str) -> String {
    format!("{}\n", text.trim_end_matches('\n'))
}

fn named(dir: &Path) -> Result<Vec<PathBuf>, String> {
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

fn files(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, String> {
    Ok(named(dir)?.into_iter().filter(|path| !hidden(path) && path.extension().is_some_and(|found| found == extension)).collect())
}

/// A reference line is a command followed by its summary; a line indented past the two spaces a new
/// command starts with continues the command above it.
fn reference_entries(dir: &Path) -> Result<Vec<String>, String> {
    let mut entries: Vec<String> = Vec::new();
    for path in files(dir, "txt")? {
        let text = read(&path)?;
        for line in text.lines() {
            if line.starts_with("   ") && entries.last().is_some() {
                let last = entries.last_mut().expect("checked");
                last.push('\n');
                last.push_str(line);
            } else if !line.trim().is_empty() {
                entries.push(line.to_owned());
            }
        }
    }
    Ok(entries)
}

/// `src/cli/reference.txt` from one file per command, each holding that command's whole reference
/// line, in file-name order (`NNNN-command.txt`).
pub fn reference(source: &Path) -> Result<String, String> {
    let mut out = block(&read(&source.join("_header.txt"))?);
    for entry in reference_entries(source)? {
        out.push_str(&block(&entry));
    }
    out.push_str(&block(&read(&source.join("_footer.txt"))?));
    Ok(out)
}

/// `src/cli/verbs.rs` from one file per verb holding its `VerbKinds` line, sorted by that line, which
/// is the order the verb table keeps.
pub fn verbs(source: &Path) -> Result<String, String> {
    let mut entries = files(source, "txt")?.into_iter().map(|path| read(&path)).collect::<Result<Vec<_>, _>>()?;
    entries.retain(|entry| !entry.trim().is_empty());
    entries.sort();
    let mut out = block(&read(&source.join("_header.txt"))?);
    for entry in entries {
        out.push_str(&block(&entry));
    }
    out.push_str(&block(&read(&source.join("_footer.txt"))?));
    Ok(out)
}

/// `structure.toml` from its tables: each per-unit file holds one table and the blank line that
/// follows it, so the files concatenate to the config byte for byte. `None` when there is no table.
pub fn structure_parts<'a>(tables: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let text: String = tables.into_iter().collect();
    (!text.trim().is_empty()).then_some(text)
}

/// `structure.toml` from one file per table in file-name order (`NNNN-name.toml`).
pub fn structure(source: &Path) -> Result<String, String> {
    let entries = files(source, "toml")?.into_iter().map(|path| read(&path)).collect::<Result<Vec<_>, _>>()?;
    structure_parts(entries.iter().map(String::as_str)).ok_or_else(|| format!("{} holds no table", source.display()))
}

#[cfg(test)]
mod tests {

    fn source(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(root.path().join(name), text).unwrap();
        }
        root
    }

    #[test]
    fn every_registry_renders_its_per_unit_tree_in_name_order() {
        let reference = source(&[("_header.txt", "Commands:\n"), ("_footer.txt", "\nOptions:\n"), ("0002-second.txt", "  second  Later\n"), ("0001-first.txt", "  first  Earlier\n"), ("_ignored.txt", "  ignored\n")]);
        assert_eq!(super::reference(reference.path()).unwrap(), "Commands:\n  first  Earlier\n  second  Later\n\nOptions:\n");
        let continued = source(&[("_header.txt", "Commands:\n"), ("_footer.txt", "\nOptions:\n"), ("0001-long.txt", "  long  Starts here\n                                                        and ends here\n")]);
        assert_eq!(super::reference(continued.path()).unwrap(), "Commands:\n  long  Starts here\n                                                        and ends here\n\nOptions:\n");
        let table = source(&[("_header.txt", "const T: &[&str] = &[\n"), ("_footer.txt", "];\n"), ("0002-late.txt", "    (\"b\", B),\n"), ("0001-early.txt", "    (\"a\", A),\n")]);
        assert_eq!(super::verbs(table.path()).unwrap(), "const T: &[&str] = &[\n    (\"a\", A),\n    (\"b\", B),\n];\n");
        let config = source(&[("0001-a.toml", "[a]\nx = 1\n\n"), ("0002-b.toml", "[[b]]\ny = 2\n")]);
        assert_eq!(super::structure(config.path()).unwrap(), "[a]\nx = 1\n\n[[b]]\ny = 2\n");
        let empty = source(&[]);
        assert!(super::structure(empty.path()).is_err());
    }
}
