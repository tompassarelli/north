use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Topic {
    pub repo: String,
    pub name: String,
    pub terms: Vec<String>,
    pub since: String,
    pub reason: String,
}

impl Topic {
    pub(crate) fn names(&self, repo: &str, title: &str) -> bool {
        let title = title.to_lowercase();
        self.repo.eq_ignore_ascii_case(repo) && self.terms.iter().any(|term| title.contains(&term.to_lowercase()))
    }

    pub(crate) fn refusal(&self) -> String {
        format!("{} is frozen in {} since {} ({}); no issue about it is filed, reopened or staffed until Tom permits it", self.name, self.repo, self.since, self.reason)
    }
}

pub(crate) fn matching<'a>(topics: &'a [Topic], repo: &str, title: &str) -> Option<&'a Topic> {
    topics.iter().find(|topic| topic.names(repo, title))
}

pub(crate) fn path() -> PathBuf {
    if let Some(path) = env::var_os("AGENTS_FROZEN_TOPICS").filter(|path| !path.is_empty()) {
        return path.into();
    }
    match env::var_os("AGENTS_OPERATOR_DIR") {
        Some(operator) => PathBuf::from(operator).join("frozen-topics.toml"),
        None => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join("code/south/main/agents/frozen-topics.toml"),
    }
}

pub(crate) fn load() -> Result<Vec<Topic>, String> {
    load_from(&path())
}

pub(crate) fn load_from(file: &Path) -> Result<Vec<Topic>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kobold() -> Vec<Topic> {
        vec![Topic { repo: "smashcraft".into(), name: "Kobold".into(), terms: vec!["kobold".into(), "forsaken paladin".into()], since: "2026-10-26".into(), reason: "being replaced".into() }]
    }

    #[test]
    fn matches_titles_case_insensitively_in_the_named_repository_only() {
        let topics = kobold();
        assert!(matching(&topics, "smashcraft", "KOBOLD rope hitbox").is_some());
        assert!(matching(&topics, "SmashCraft", "Fix the Forsaken Paladin shield").is_some());
        assert!(matching(&topics, "wisp", "kobold").is_none());
        assert!(matching(&topics, "smashcraft", "Anub'arak scorecard").is_none());
    }

    #[test]
    fn reads_the_list_file_and_reports_a_missing_or_invalid_one() {
        let dir = std::env::temp_dir().join(format!("north-frozen-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("frozen-topics.toml");
        fs::write(&file, "[[topic]]\nrepo = \"smashcraft\"\nname = \"Kobold\"\nterms = [\"kobold\"]\nsince = \"2026-10-26\"\nreason = \"x\"\n").unwrap();
        assert_eq!(load_from(&file).unwrap().len(), 1);
        fs::write(&file, "[[topic]]\nrepo = 3\n").unwrap();
        assert!(load_from(&file).unwrap_err().contains("invalid"));
        assert!(load_from(&dir.join("absent.toml")).unwrap_err().contains("unread"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
