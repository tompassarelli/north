use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Workspace {
    pub(super) workspace: String,
    pub(super) leaf: String,
    pub(super) hub: String,
    pub(super) threshold_seconds: Option<f64>,
}

pub(super) fn read(repo: &Path) -> Result<Vec<Workspace>, String> {
    let path = repo.join("structure.toml");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn parse(text: &str) -> Result<Vec<Workspace>, String> {
    let document: toml_edit::DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
    let Some(item) = document.get("build_time") else { return Ok(Vec::new()) };
    let tables = item.as_array_of_tables().ok_or("build_time must be an array of tables ([[build_time]])")?;
    tables.iter().enumerate().map(|(index, table)| entry(index, table)).collect()
}

fn entry(index: usize, table: &toml_edit::Table) -> Result<Workspace, String> {
    for (key, _) in table {
        if !["workspace", "leaf", "hub", "test_no_run_seconds"].contains(&key) {
            return Err(format!("build_time {}: unknown key {key}", index + 1));
        }
    }
    let text = |key: &str| table.get(key).and_then(|item| item.as_str()).map(str::to_owned).ok_or(format!("build_time {}: {key} must be a string", index + 1));
    let threshold_seconds = match table.get("test_no_run_seconds") {
        None => None,
        Some(item) => Some(item.as_float().or_else(|| item.as_integer().map(|n| n as f64)).ok_or(format!("build_time {}: test_no_run_seconds must be a number", index + 1))?),
    };
    Ok(Workspace { workspace: text("workspace")?, leaf: text("leaf")?, hub: text("hub")?, threshold_seconds })
}
