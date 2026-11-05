use std::fs;
use std::path::{Path, PathBuf};

use super::docs::Index;

const HEADER_FILE: &str = "_header.md";
const TITLE_FILE: &str = "_title.md";

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn block(text: &str) -> String {
    format!("{}\n", text.trim_end_matches('\n'))
}

fn is_item(block: &str) -> bool {
    let line = block.trim_start();
    line.starts_with("- ") || line.starts_with("* ")
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

fn section(dir: &Path) -> Result<String, String> {
    let mut out = format!("{}\n", block(&read(&dir.join(TITLE_FILE))?));
    let mut previous: Option<String> = None;
    for path in sorted(dir)?.into_iter().filter(|path| path.extension().is_some_and(|extension| extension == "md") && !hidden(path)) {
        let entry = block(&read(&path)?);
        if previous.as_ref().is_some_and(|previous| !(is_item(previous) && is_item(&entry))) {
            out.push('\n');
        }
        out.push_str(&entry);
        previous = Some(entry);
    }
    Ok(out)
}

pub fn render(source: &Path) -> Result<String, String> {
    let mut parts = vec![block(&read(&source.join(HEADER_FILE))?)];
    for path in sorted(source)?.into_iter().filter(|path| path.is_dir() && !hidden(path)) {
        parts.push(section(&path)?);
    }
    Ok(parts.join("\n"))
}

pub const INDEX: Index = Index { command: "features-index", source_dir: "docs/features", file: "docs/features.md", render };

#[cfg(test)]
mod tests;
