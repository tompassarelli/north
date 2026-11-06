use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml_edit::DocumentMut;

const DEFAULTS: &str = "harness/defaults.toml";
const SAFE_PUSH: &str = ".safe-push";
const SAFE_PUSH_READERS: &[&str] = &["bin/safe-push", "crates/north-landing/", "crates/north-safe-push/"];
const CODE: &[(&str, &str)] = &[("src/", ".rs"), ("crates/", ".rs"), ("harness/", ".ts"), ("agent-machinery/", ".ts")];

#[derive(Debug, Eq, PartialEq)]
pub struct MissingKey {
    pub path: String,
    pub line: usize,
    pub name: String,
}

#[derive(Default)]
pub struct Owners {
    pub defaults: BTreeMap<String, BTreeSet<String>>,
    pub safe_push: BTreeMap<String, BTreeSet<String>>,
    pub safe_push_read: String,
    pub code_words: BTreeSet<String>,
}

impl Owners {
    fn safe_push_has(&self, section: &str, key: &str) -> bool {
        self.safe_push.get(section).is_some_and(|keys| keys.contains(key)) || self.safe_push_read.contains(&format!("{section}.{key}"))
    }

    fn read_by_code(&self, table: &str, key: Option<&str>) -> bool {
        table.split('.').chain(key).all(|word| self.code_words.contains(word))
    }

    fn table_exists(&self, table: &str, key: Option<&str>) -> bool {
        if let Some(keys) = self.defaults.get(table) {
            return key.is_none_or(|key| keys.contains(key));
        }
        if self.safe_push.contains_key(table) {
            return key.is_none_or(|key| self.safe_push_has(table, key));
        }
        self.read_by_code(table, key)
    }
}

fn word(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase()) && text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn table_citation(span: &str) -> Option<(&str, Option<&str>)> {
    let rest = span.strip_prefix('[')?;
    let (table, key) = rest.split_once(']')?;
    let key = match key {
        "" => None,
        key => Some(key.strip_prefix(' ').filter(|key| word(key))?),
    };
    table.split('.').all(word).then_some((table, key))
}

fn dotted_citation<'a>(span: &'a str, owners: &Owners) -> Option<(&'a str, &'a str)> {
    let (section, key) = span.split_once('.')?;
    (owners.safe_push.contains_key(section) && word(key)).then_some((section, key))
}

pub fn scan(path: &str, text: &str, owners: &Owners, known: &BTreeSet<String>) -> Vec<MissingKey> {
    let mut fenced = false;
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for span in super::spans(line) {
            let missing = if let Some((table, key)) = table_citation(span) {
                !owners.table_exists(table, key)
            } else if let Some((section, key)) = dotted_citation(span, owners) {
                !known.contains(span) && !owners.safe_push_has(section, key)
            } else {
                false
            };
            if missing {
                found.push(MissingKey { path: path.into(), line: index + 1, name: span.into() });
            }
        }
    }
    found
}

fn defaults(root: &Path) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let text = std::fs::read_to_string(root.join(DEFAULTS)).unwrap_or_default();
    let document: DocumentMut = text.parse().map_err(|e| format!("{DEFAULTS}: {e}"))?;
    Ok(document
        .iter()
        .filter_map(|(name, item)| Some((name.to_owned(), item.as_table_like()?.iter().map(|(key, _)| key.to_owned()).collect())))
        .collect())
}

fn safe_push(root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let mut sections: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let text = std::fs::read_to_string(root.join(SAFE_PUSH)).unwrap_or_default();
    let mut section = None;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            section = Some(name.trim().to_owned());
            sections.entry(name.trim().to_owned()).or_default();
        } else if let (Some(section), Some((key, _))) = (&section, line.split_once('=')) {
            sections.entry(section.clone()).or_default().insert(key.trim().to_owned());
        }
    }
    sections
}

fn code_words(text: &str, into: &mut BTreeSet<String>) {
    into.extend(text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).filter(|token| word(token)).map(str::to_owned));
}

pub fn owners(root: &Path, files: &[String]) -> Result<Owners, String> {
    let mut owners = Owners { defaults: defaults(root)?, safe_push: safe_push(root), ..Owners::default() };
    for path in files {
        let code = CODE.iter().any(|(prefix, extension)| path.starts_with(prefix) && path.ends_with(extension));
        let reader = SAFE_PUSH_READERS.iter().any(|prefix| path.starts_with(prefix));
        if !(code || reader) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(path)) else { continue };
        if code {
            code_words(&text, &mut owners.code_words);
        }
        if reader {
            owners.safe_push_read.push_str(&text);
        }
    }
    Ok(owners)
}
