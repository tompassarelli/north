use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use super::session_scope::COORDINATOR_ROLE;

pub const FILE: &str = "assigned";

#[derive(Debug, PartialEq, Eq)]
pub enum Checkable {
    Any,
    Issues(Vec<String>),
}

pub fn assigned(directory: &Path) -> Vec<String> {
    fs::read_to_string(directory.join(FILE)).unwrap_or_default().lines().map(str::trim).filter(|issue| !issue.is_empty()).map(str::to_owned).collect()
}

pub fn assign(directory: &Path, issue: &str, held: bool) -> io::Result<()> {
    let mut issues: Vec<String> = assigned(directory).into_iter().filter(|kept| kept != issue).collect();
    if held {
        issues.push(issue.to_owned());
    }
    let staged = directory.join(format!(".{FILE}.{}", std::process::id()));
    fs::write(&staged, issues.iter().map(|issue| format!("{issue}\n")).collect::<String>())?;
    fs::rename(&staged, directory.join(FILE))
}

pub fn coordinator(record: &Value) -> bool {
    match record["role"].as_str() {
        Some(role) => north_shared_config::roles::is_coordinator_role(role),
        None => record["name"].as_str() == Some(COORDINATOR_ROLE) && record["parent"].is_null() && record["task_ref"].is_null(),
    }
}

pub fn checkable(directory: &Path, record: &Value) -> Checkable {
    if coordinator(record) {
        return Checkable::Any;
    }
    Checkable::Issues(record["task_ref"].as_str().filter(|task| !task.is_empty()).map(str::to_owned).into_iter().chain(assigned(directory)).collect())
}
