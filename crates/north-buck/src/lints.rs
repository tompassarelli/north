use std::path::Path;

use toml_edit::{DocumentMut, Item};

pub fn workspace_table(root: &Path) -> Result<Option<Item>, String> {
    let document = read(&root.join("Cargo.toml"))?;
    Ok(document.get("workspace").and_then(|workspace| workspace.get("lints")).cloned())
}

pub fn flags(root: &Path, dir: &str, workspace: Option<&Item>) -> Result<Vec<String>, String> {
    let manifest = root.join(dir).join("Cargo.toml");
    let document = read(&manifest)?;
    let Some(own) = document.get("lints") else {
        return Ok(Vec::new());
    };
    let table = if own.get("workspace").and_then(Item::as_bool) == Some(true) {
        workspace.ok_or_else(|| format!("{}: lints.workspace = true but the workspace has no [workspace.lints]", manifest.display()))?
    } else {
        own
    };
    let mut lints = Vec::new();
    for (tool, entries) in table.as_table_like().into_iter().flat_map(|table| table.iter()) {
        let prefix = if tool == "rust" { String::new() } else { format!("{tool}::") };
        for (name, value) in entries.as_table_like().into_iter().flat_map(|entries| entries.iter()) {
            let (level, priority) = level(value).ok_or_else(|| format!("{}: lint {prefix}{name} has no level", manifest.display()))?;
            lints.push((priority, format!("{prefix}{name}"), level));
        }
    }
    lints.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));
    Ok(lints.into_iter().map(|(_, name, level)| format!("--{level}={name}")).collect())
}

fn level(value: &Item) -> Option<(String, i64)> {
    if let Some(level) = value.as_str() {
        return Some((level.to_owned(), 0));
    }
    let table = value.as_table_like()?;
    let level = table.get("level")?.as_str()?.to_owned();
    Some((level, table.get("priority").and_then(Item::as_integer).unwrap_or(0)))
}

fn read(path: &Path) -> Result<DocumentMut, String> {
    let text = std::fs::read_to_string(path).map_err(|why| format!("{}: {why}", path.display()))?;
    text.parse().map_err(|why| format!("{}: {why}", path.display()))
}
