use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::docs::Index;

const README: &str = "README.md";
const KINDS: &str = "kinds.toml";
const HEADER: &str = "# Rationale notes\n\nNothing here is needed to use north: these notes record why it is built the way it is. `north docs rationale-index` writes this page from each note's title, its first sentence that is not an owner line or a date, and its kind in `kinds.toml`; edit the notes and that file, not this page.\n";
const UNSORTED: &str = "Not yet sorted";
const GROUPS: [(&str, &str); 6] = [
    ("mechanism", "Live mechanisms"),
    ("plan", "Plans"),
    ("measurement", "Measurements"),
    ("decision", "Decision records"),
    ("review", "Reviews of outside sources"),
    ("retired", "Retired designs"),
];
const DATED_OPENINGS: [&str; 10] = ["Plan", "Decided", "Measured", "Written", "Recorded", "Revised", "Reviewed", "Review", "Audit", "Updated"];

pub const INDEX: Index = Index { command: "rationale-index", source_dir: "docs/rationale", file: "docs/rationale/README.md", render, notes: |_| Ok(Vec::new()) };

fn title(text: &str) -> Option<&str> {
    text.lines().next()?.strip_prefix("# ").map(str::trim).filter(|title| !title.is_empty())
}

fn paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines().skip(1).chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line.trim());
        }
    }
    paragraphs.retain(|paragraph| !["#", "```", "|", "<"].iter().any(|opening| paragraph.starts_with(opening)));
    paragraphs
}

fn sentences(paragraph: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut characters = paragraph.char_indices().peekable();
    while let Some((at, character)) = characters.next() {
        let ends = matches!(character, '.' | '!' | '?') && characters.peek().is_none_or(|(_, next)| next.is_whitespace());
        if ends {
            found.push(paragraph[start..at + character.len_utf8()].trim());
            start = at + character.len_utf8();
        }
    }
    let rest = paragraph[start..].trim();
    if !rest.is_empty() {
        found.push(rest);
    }
    found
}

fn names_a_date(word: &str) -> bool {
    let digits = word.trim_matches(|character: char| !character.is_ascii_digit());
    digits.len() >= 10 && digits.as_bytes()[..10].iter().enumerate().all(|(at, byte)| if at == 4 || at == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
}

fn summary_of(sentence: &str) -> Option<String> {
    if sentence.starts_with("Owner:") {
        return None;
    }
    let opening = sentence.split_whitespace().next().unwrap_or_default();
    if !DATED_OPENINGS.contains(&opening) || !sentence.split_whitespace().any(names_a_date) {
        return Some(sentence.to_owned());
    }
    let (_, rest) = sentence.split_once(": ")?;
    let mut characters = rest.chars();
    characters.next().map(|first| first.to_uppercase().chain(characters).collect())
}

fn summary(text: &str) -> Option<String> {
    let paragraphs = paragraphs(text);
    let first = sentences(paragraphs.first()?).first().map(|sentence| (*sentence).to_owned())?;
    Some(paragraphs.iter().flat_map(|paragraph| sentences(paragraph)).find_map(summary_of).unwrap_or(first))
}

fn line(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| format!("{} is not UTF-8", path.display()))?;
    let title = title(&text).ok_or_else(|| format!("{}: the first line is not a `# ` title", path.display()))?;
    let summary = summary(&text).ok_or_else(|| format!("{}: no paragraph follows the title", path.display()))?;
    Ok(format!("- [{title}]({name}): {summary}\n"))
}

fn kinds(source: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = source.join(KINDS);
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(BTreeMap::new());
    };
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|error| format!("{}: {error}", path.display()))?;
    document
        .iter()
        .map(|(note, kind)| match kind.as_str() {
            Some(kind) if GROUPS.iter().any(|(key, _)| *key == kind) => Ok((note.to_owned(), kind.to_owned())),
            _ => Err(format!("{}: `{note}` must be one of {}", path.display(), GROUPS.map(|(key, _)| format!("\"{key}\"")).join(", "))),
        })
        .collect()
}

pub fn render(source: &Path) -> Result<String, String> {
    let mut notes = fs::read_dir(source)
        .map_err(|error| format!("{}: {error}", source.display()))?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|error| format!("{}: {error}", source.display())))
        .collect::<Result<Vec<_>, _>>()?;
    notes.retain(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "md") && path.file_name().is_some_and(|name| name != README));
    notes.sort();
    let kinds = kinds(source)?;
    let mut groups: BTreeMap<usize, String> = BTreeMap::new();
    for note in &notes {
        let stem = note.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
        let group = kinds.get(stem).and_then(|kind| GROUPS.iter().position(|(key, _)| *key == kind.as_str())).unwrap_or(GROUPS.len());
        groups.entry(group).or_default().push_str(&line(note)?);
    }
    let mut out = HEADER.to_owned();
    for (group, lines) in groups {
        let heading = GROUPS.get(group).map_or(UNSORTED, |(_, heading)| heading);
        out.push_str(&format!("\n## {heading}\n\n{lines}"));
    }
    Ok(out)
}
