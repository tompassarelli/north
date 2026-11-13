use std::fs;
use std::path::{Path, PathBuf};

use crate::docs::Index;

const HEADER_FILE: &str = "_header.txt";
const FOOTER_FILE: &str = "_footer.txt";
const TITLE_FILE: &str = "_title.txt";
const NAME_FIELD: usize = 22;
const SUMMARY_LIMIT: usize = 70;

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

fn sections(source: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(sorted(source)?.into_iter().filter(|path| path.is_dir() && !hidden(path)).collect())
}

fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(sorted(dir)?.into_iter().filter(|path| !hidden(path) && path.extension().is_some_and(|extension| extension == "txt")).collect())
}

/// One help line: the first line names the command and the rest is its summary. The renderer lays
/// the name out in the fixed column every `north --help` line uses.
fn line(path: &Path) -> Result<String, String> {
    let text = read(path)?;
    let mut lines = text.lines();
    let name = lines.next().map(str::trim).filter(|name| !name.is_empty()).ok_or_else(|| format!("{}: the first line names no command", path.display()))?;
    let summary = lines.map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        return Err(format!("{}: {name} has no summary", path.display()));
    }
    Ok(format!("  {name:<NAME_FIELD$}{summary}\n"))
}

fn section(dir: &Path) -> Result<String, String> {
    let mut out = block(&read(&dir.join(TITLE_FILE))?);
    for path in entries(dir)? {
        out.push_str(&line(&path)?);
    }
    Ok(out)
}

pub fn render(source: &Path) -> Result<String, String> {
    let mut parts = vec![block(&read(&source.join(HEADER_FILE))?)];
    for path in sections(source)? {
        parts.push(section(&path)?);
    }
    parts.push(block(&read(&source.join(FOOTER_FILE))?));
    Ok(parts.join("\n"))
}

fn notes(source: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for dir in sections(source)? {
        for path in entries(&dir)? {
            let text = read(&path)?;
            let name = text.lines().next().unwrap_or_default().trim();
            let summary = text.lines().skip(1).map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
            let length = summary.chars().count();
            if summary.contains('|') {
                notes.push(format!("{}: the summary holds '|'; list its subcommands in `north {name} --help`", path.display()));
            } else if length > SUMMARY_LIMIT {
                notes.push(format!("{}: the summary is {length} characters, over {SUMMARY_LIMIT}", path.display()));
            }
        }
    }
    Ok(notes)
}

pub const INDEX: Index = Index { command: "help-index", source_dir: "src/cli/help", file: "src/cli/help.txt", render, notes };
