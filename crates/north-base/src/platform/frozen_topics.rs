use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, PartialEq)]
pub struct Topic {
    pub repo: String,
    pub name: String,
    pub terms: Vec<String>,
    pub since: String,
    pub reason: String,
}

impl Topic {
    pub fn names(&self, repo: &str, title: &str) -> bool {
        let title = title.to_lowercase();
        self.repo.eq_ignore_ascii_case(repo) && self.terms.iter().any(|term| title.contains(&term.to_lowercase()))
    }

    pub fn refusal(&self) -> String {
        format!("{} is frozen in {} since {} ({}); no issue about it is filed, reopened or staffed until Tom permits it", self.name, self.repo, self.since, self.reason)
    }
}

pub fn matching<'a>(topics: &'a [Topic], repo: &str, title: &str) -> Option<&'a Topic> {
    topics.iter().find(|topic| topic.names(repo, title))
}

pub fn path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTS_FROZEN_TOPICS").filter(|path| !path.is_empty()) {
        return path.into();
    }
    match env::var_os("AGENTS_OPERATOR_DIR") {
        Some(operator) => PathBuf::from(operator).join("frozen-topics.toml"),
        None => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join("code/south/main/agents/frozen-topics.toml"),
    }
}

pub fn load() -> Result<Vec<Topic>, String> {
    load_from(&path())
}

pub fn load_from(file: &Path) -> Result<Vec<Topic>, String> {
    let text = fs::read_to_string(file).map_err(|error| format!("frozen topics unread at {}: {error}", file.display()))?;
    let invalid = |why: String| format!("frozen topics invalid at {}: {why}", file.display());
    let document: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| invalid(error.to_string()))?;
    let Some(tables) = document.get("topic").and_then(Item::as_array_of_tables) else {
        return Ok(Vec::new());
    };
    tables
        .iter()
        .map(|table| {
            let text = |key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned).ok_or_else(|| invalid(format!("a topic lacks `{key}`")));
            let terms = table.get("terms").and_then(Item::as_array).ok_or_else(|| invalid("a topic lacks `terms`".into()))?;
            Ok(Topic {
                repo: text("repo")?,
                name: text("name")?,
                terms: terms.iter().filter_map(|term| term.as_str().map(str::to_owned)).collect(),
                since: text("since")?,
                reason: text("reason")?,
            })
        })
        .collect()
}
