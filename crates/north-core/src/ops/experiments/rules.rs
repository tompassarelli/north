//! The review rules rationale notes carry, and the check that the registry carries each of them.
use std::fs;
use std::path::Path;

use super::registry::{self, Experiment, collapse};

pub const NOTES: &str = "docs/rationale";
const OVERTURNED: &str = "Overturned by:";
const REVIEW: &str = "## Review";

/// A review rule: an `Overturned by:` paragraph or the body of a `## Review` section, collapsed to
/// one line so a registry entry can quote it exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub note: String,
    pub line: usize,
    pub text: String,
}

pub fn rules(note: &str, text: &str) -> Vec<Rule> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let body: Vec<&str> = if line.starts_with(OVERTURNED) {
            lines[index..].iter().take_while(|line| !line.trim().is_empty()).copied().collect()
        } else if line.trim_end() == REVIEW {
            lines[index + 1..].iter().take_while(|line| !line.starts_with('#')).copied().collect()
        } else {
            continue;
        };
        found.push(Rule { note: note.to_owned(), line: index + 1, text: collapse(&body.join(" ")) });
    }
    found
}

/// Every rule in the notes directly under `root/docs/rationale`, in file then line order.
pub fn read(root: &Path) -> Result<Vec<Rule>, String> {
    let directory = root.join(NOTES);
    let mut notes: Vec<_> = fs::read_dir(&directory)
        .map_err(|why| format!("{}: {why}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    notes.sort();
    let mut found = Vec::new();
    for path in notes {
        let text = fs::read_to_string(&path).map_err(|why| format!("{}: {why}", path.display()))?;
        let name = path.file_name().map(|name| format!("{NOTES}/{}", name.to_string_lossy())).unwrap_or_default();
        found.extend(rules(&name, &text));
    }
    Ok(found)
}

/// What stops a landing: a rule no entry quotes exactly, or an entry quoting a rule its note no
/// longer holds.
pub fn uncovered(rules: &[Rule], entries: &[Experiment]) -> Vec<String> {
    let quoted = |rule: &Rule| entries.iter().any(|entry| entry.cited.as_ref().is_some_and(|cited| cited.note == rule.note && cited.rule == rule.text));
    let held = |entry: &Experiment| entry.cited.as_ref().is_none_or(|cited| rules.iter().any(|rule| cited.note == rule.note && cited.rule == rule.text));
    let missing = rules.iter().filter(|rule| !quoted(rule)).map(|rule| {
        format!("{}:{}: review rule has no entry in {} quoting it exactly (add one, or update the entry whose rule changed): {}", rule.note, rule.line, registry::PATH, rule.text)
    });
    let stale = entries.iter().filter(|entry| !held(entry)).map(|entry| {
        let cited = entry.cited.as_ref().map_or_else(String::new, |cited| cited.note.clone());
        format!("{}: entry {} quotes a rule {cited} no longer holds; quote the note's current rule", registry::PATH, entry.id)
    });
    missing.chain(stale).collect()
}

/// The land check: the registry parses, and it and the notes agree.
pub fn check(root: &Path) -> Vec<String> {
    let path = root.join(registry::PATH);
    let entries = match fs::read_to_string(&path).map_err(|why| vec![format!("{}: {why}", path.display())]).and_then(|text| registry::parse(&text)) {
        Ok(entries) => entries,
        Err(problems) => return problems,
    };
    match read(root) {
        Ok(rules) => uncovered(&rules, &entries),
        Err(why) => vec![why],
    }
}
