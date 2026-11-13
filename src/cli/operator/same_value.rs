//! `north config operator check DIR --distinct-from OWNER`: a template value that equals the owner's value at the same key is refused.
//! Only strings at the keys of `PRIVATE_KEYS` are compared, because every other value is a shared vocabulary word or a number that a template legitimately repeats; a finding names the template file and key and never the value.
use std::collections::BTreeSet;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Value};

const PRIVATE_KEYS: &[&str] = &["id", "name", "email", "emails", "claude_org", "person", "repo", "repos", "path", "host", "host_config", "deploy_ssh", "ssh", "agent_ssh", "source", "location", "use", "reason", "readout", "decision", "evidence", "record"];

type Scalar = (String, String, String);

fn scalars(item: &Item, label: &str, key: &str, found: &mut Vec<Scalar>) {
    match item {
        Item::Table(table) => table.iter().for_each(|(name, inner)| scalars(inner, &format!("{label}.{name}"), name, found)),
        Item::ArrayOfTables(tables) => {
            for (index, table) in tables.iter().enumerate() {
                table.iter().for_each(|(name, inner)| scalars(inner, &format!("{label}[{index}].{name}"), name, found));
            }
        }
        Item::Value(value) => value_scalars(value, label, key, found),
        Item::None => {}
    }
}

fn value_scalars(value: &Value, label: &str, key: &str, found: &mut Vec<Scalar>) {
    let text = match value {
        Value::String(text) => text.value().clone(),
        Value::Integer(_) | Value::Float(_) | Value::Datetime(_) | Value::Boolean(_) => return,
        Value::Array(items) => {
            items.iter().enumerate().for_each(|(index, inner)| value_scalars(inner, &format!("{label}[{index}]"), key, found));
            return;
        }
        Value::InlineTable(table) => {
            table.iter().for_each(|(name, inner)| value_scalars(inner, &format!("{label}.{name}"), name, found));
            return;
        }
    };
    if !text.is_empty() && PRIVATE_KEYS.contains(&key) {
        found.push((label.to_owned(), key.to_owned(), text));
    }
}

fn read(root: &Path, file: &str) -> Result<Vec<Scalar>, String> {
    let path = root.join(file);
    let source = std::fs::read_to_string(&path).map_err(|error| format!("{file}: file: cannot read {}: {error}", path.display()))?;
    let document: DocumentMut = source.parse().map_err(|error: toml_edit::TomlError| format!("{file}: file: {}", error.to_string().replace('\n', " ")))?;
    let mut found = Vec::new();
    for (name, item) in document.iter() {
        scalars(item, name, name, &mut found);
    }
    Ok(found)
}

pub fn findings(template: &Path, owner: &Path, files: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for file in files {
        match (read(template, file), read(owner, file)) {
            (Ok(mine), Ok(theirs)) => {
                let private: BTreeSet<(String, String)> = theirs.into_iter().map(|(_, key, value)| (key, value)).collect();
                found.extend(mine.into_iter().filter(|(_, key, value)| private.contains(&(key.clone(), value.clone()))).map(|(label, ..)| format!("{file}: {label}: equals the owner's value at this key")));
            }
            (Err(error), _) | (_, Err(error)) => found.push(error),
        }
    }
    found
}
