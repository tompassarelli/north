use std::fs;
use std::path::Path;

use super::docs::Index;

const README: &str = "README.md";
const HEADER: &str = "# Rationale notes\n\nWhy north is built the way it is, one note per decision. `north docs rationale-index` writes this list from each note's title and first sentence; edit the notes, not this file.\n";

pub const INDEX: Index = Index { command: "rationale-index", source_dir: "docs/rationale", file: "docs/rationale/README.md", render, lint: |_| Ok(()) };

fn title(text: &str) -> Option<&str> {
    text.lines().next()?.strip_prefix("# ").map(str::trim).filter(|title| !title.is_empty())
}

fn first_paragraph(text: &str) -> Option<String> {
    let mut lines = text.lines().skip(1).skip_while(|line| line.trim().is_empty() || line.starts_with('#'));
    let first = lines.next()?;
    let paragraph = std::iter::once(first).chain(lines.take_while(|line| !line.trim().is_empty())).map(str::trim).collect::<Vec<_>>();
    Some(paragraph.join(" "))
}

fn first_sentence(paragraph: &str) -> &str {
    let mut characters = paragraph.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        let ends = matches!(character, '.' | '!' | '?') && characters.peek().is_none_or(|(_, next)| next.is_whitespace());
        if ends {
            return &paragraph[..at + character.len_utf8()];
        }
    }
    paragraph
}

fn line(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| format!("{} is not UTF-8", path.display()))?;
    let title = title(&text).ok_or_else(|| format!("{}: the first line is not a `# ` title", path.display()))?;
    let paragraph = first_paragraph(&text).ok_or_else(|| format!("{}: no paragraph follows the title", path.display()))?;
    Ok(format!("- [{title}]({name}): {}\n", first_sentence(&paragraph)))
}

pub fn render(source: &Path) -> Result<String, String> {
    let mut notes = fs::read_dir(source)
        .map_err(|error| format!("{}: {error}", source.display()))?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|error| format!("{}: {error}", source.display())))
        .collect::<Result<Vec<_>, _>>()?;
    notes.retain(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "md") && path.file_name().is_some_and(|name| name != README));
    notes.sort();
    let mut out = format!("{HEADER}\n");
    for note in &notes {
        out.push_str(&line(note)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
