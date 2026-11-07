use std::fs;
use std::path::{Path, PathBuf};

use super::docs::Index;

const HEADER_FILE: &str = "_header.md";
const TITLE_FILE: &str = "_title.md";
const FIRST_SENTENCE_CAP: usize = 600;
const CLIPPED: char = '…';

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

fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(sorted(dir)?.into_iter().filter(|path| path.extension().is_some_and(|extension| extension == "md") && !hidden(path)).collect())
}

fn sections(source: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(sorted(source)?.into_iter().filter(|path| path.is_dir() && !hidden(path)).collect())
}

fn first_sentence(item: &str) -> &str {
    let chars: Vec<(usize, char)> = item.char_indices().collect();
    let mut quoted = false;
    for (index, &(at, found)) in chars.iter().enumerate() {
        match found {
            '`' => quoted = !quoted,
            '.' if !quoted => {
                let (next, after) = (chars.get(index + 1).map(|pair| pair.1), chars.get(index + 2).map(|pair| pair.1));
                if next.is_none_or(|next| next == ' ' && after.is_some_and(|after| after.is_uppercase() || after == '`' || after == '(')) {
                    return &item[..=at];
                }
            }
            _ => {}
        }
    }
    item
}

fn clipped(sentence: &str) -> String {
    if sentence.chars().count() <= FIRST_SENTENCE_CAP {
        return sentence.to_owned();
    }
    let head: String = sentence.chars().take(FIRST_SENTENCE_CAP - 1).collect();
    let cut = head.rfind(' ').filter(|space| *space > head.len() / 2).map_or(head.as_str(), |space| &head[..space]);
    format!("{}{CLIPPED}", cut.trim_end_matches([',', ';', ':']))
}

fn summary(entry: &str, link: &str) -> String {
    let mut shortened = false;
    let mut lines: Vec<String> = entry
        .lines()
        .map(|line| {
            if !is_item(line) {
                return line.to_owned();
            }
            let shown = clipped(first_sentence(line));
            shortened |= shown != line;
            shown
        })
        .collect();
    if shortened && let Some(last) = lines.iter_mut().rev().find(|line| is_item(line)) {
        last.push_str(&format!(" [more]({link})"));
    }
    lines.join("\n")
}

fn link(source: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(source).unwrap_or(path).to_string_lossy();
    format!("{}/{relative}", source.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()))
}

fn section(source: &Path, dir: &Path) -> Result<String, String> {
    let mut out = format!("{}\n", block(&read(&dir.join(TITLE_FILE))?));
    let mut previous: Option<String> = None;
    for path in entries(dir)? {
        let entry = block(&summary(&read(&path)?, &link(source, &path)));
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
    for path in sections(source)? {
        parts.push(section(source, &path)?);
    }
    Ok(parts.join("\n"))
}

fn notes(source: &Path) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for dir in sections(source)? {
        for path in entries(&dir)? {
            for line in read(&path)?.lines().filter(|line| is_item(line)) {
                let length = first_sentence(line).chars().count();
                if length > FIRST_SENTENCE_CAP {
                    notes.push(format!("docs/{}: first sentence {length} characters, {} over the {FIRST_SENTENCE_CAP}-character cap; clipped in the index", link(source, &path), length - FIRST_SENTENCE_CAP));
                }
            }
        }
    }
    Ok(notes)
}

pub const INDEX: Index = Index { command: "features-index", source_dir: "docs/features", file: "docs/features.md", render, notes };
