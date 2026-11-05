use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

static CREATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)CREATE TABLE (?:IF NOT EXISTS )?["`]?([a-z_][a-z0-9_]*)"#).expect("create table pattern"));
static ALTERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)ALTER TABLE ["`]?([a-z_][a-z0-9_]*)"#).expect("alter table pattern"));
static NAMED_TABLES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"((?:`[a-z_][a-z0-9_]*`(?:,? and |, ))*`[a-z_][a-z0-9_]*`) tables?\b").expect("named table pattern"));
static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([a-z_][a-z0-9_]*)`").expect("name pattern"));
static TABLE_COLUMN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([a-z_][a-z0-9_]*)\.([a-z_][a-z0-9_]*)`").expect("table column pattern"));

const FILE_EXTENSIONS: &[&str] = &["rs", "md", "toml", "sh", "py", "json", "jsonl", "nix", "sql", "lock", "txt", "yml", "yaml", "db", "log", "ts", "js"];

#[derive(Debug, Eq, PartialEq)]
pub struct CitedMetric {
    pub line: usize,
    pub table: String,
    pub column: Option<String>,
    pub defined_in: Option<String>,
}

fn schema_source(path: &str) -> bool {
    path.ends_with(".rs") || path.ends_with(".sql") || path.ends_with(".py") || (path.starts_with("bin/") && !path.contains('.'))
}

struct Tables {
    root: std::path::PathBuf,
    defined: BTreeMap<String, BTreeSet<String>>,
}

impl Tables {
    fn read(root: &Path, files: &[String]) -> Self {
        let mut defined: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for file in files.iter().filter(|file| schema_source(file) && !file.contains("/fixtures/")) {
            let Ok(text) = std::fs::read_to_string(root.join(file)) else { continue };
            for table in CREATED.captures_iter(&text).chain(ALTERED.captures_iter(&text)).map(|found| found[1].to_lowercase()) {
                defined.entry(table).or_default().insert(file.clone());
            }
        }
        Self { root: root.to_path_buf(), defined }
    }

    fn table(&self, table: &str) -> Option<String> {
        self.defined.get(table).and_then(|files| files.first().cloned())
    }

    fn column(&self, table: &str, column: &str) -> Option<String> {
        let word = Regex::new(&format!(r"\b{}\b", regex::escape(column))).ok()?;
        self.defined.get(table)?.iter().find(|file| std::fs::read_to_string(self.root.join(file)).is_ok_and(|text| word.is_match(&text))).cloned()
    }

    fn cited(&self, text: &str) -> Vec<CitedMetric> {
        let mut found = Vec::new();
        let mut fenced = false;
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            for named in NAMED_TABLES.captures_iter(line) {
                for table in NAME.captures_iter(&named[1]).map(|found| found[1].to_owned()) {
                    let defined_in = self.table(&table);
                    found.push(CitedMetric { line: index + 1, table, column: None, defined_in });
                }
            }
            for cited in TABLE_COLUMN.captures_iter(line).filter(|cited| self.defined.contains_key(&cited[1]) && !FILE_EXTENSIONS.contains(&&cited[2])) {
                let defined_in = self.column(&cited[1], &cited[2]);
                found.push(CitedMetric { line: index + 1, table: cited[1].to_owned(), column: Some(cited[2].to_owned()), defined_in });
            }
        }
        found
    }
}

pub fn cited_metrics(root: &Path, text: &str) -> Result<Vec<CitedMetric>, String> {
    Ok(Tables::read(root, &super::all_tracked(root)?).cited(text))
}
