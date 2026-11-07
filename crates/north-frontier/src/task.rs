use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::refusal;
use north_base::error::NorthError;

/// One frozen box: the landing it reproduces, the brief its worker got and the tests that grade it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    pub id: String,
    pub repo: String,
    pub issue: String,
    pub run: String,
    pub base: String,
    pub landed: String,
    pub category: String,
    pub spec: String,
    pub model: String,
    pub effort: String,
    pub brief_sha256: String,
    pub hidden: Vec<String>,
    pub targets: Vec<Target>,
}

/// A slice of the test suite: a cargo package and a nextest filterset within it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub package: String,
    pub filter: String,
}

pub fn load(path: &Path) -> Result<Vec<Task>, NorthError> {
    let text = fs::read_to_string(path).map_err(NorthError::Io)?;
    serde_json::from_str(&text).map_err(NorthError::Json)
}

pub fn chosen<'a>(tasks: &'a [Task], only: &[String]) -> Vec<&'a Task> {
    tasks.iter().filter(|task| only.is_empty() || only.contains(&task.id)).collect()
}

pub fn brief(dir: &Path, task: &Task) -> Result<String, NorthError> {
    let text = fs::read_to_string(dir.join(format!("{}.md", task.id))).map_err(NorthError::Io)?;
    if sha256(&text) != task.brief_sha256 {
        return Err(refusal(format!("{}: the brief in {} is not the frozen one", task.id, dir.display())));
    }
    Ok(text)
}

pub fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn is_test_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.ends_with(".rs") && (path.starts_with("tests/") || path.contains("/tests/") || name == "tests.rs" || name.ends_with("_tests.rs"))
}

/// The nextest slice a test file belongs to: a crate's unit tests, or one module of an integration binary.
pub fn target_of(path: &str) -> Option<Target> {
    let (package, rest) = match path.strip_prefix("crates/") {
        Some(inner) => {
            let (krate, rest) = inner.split_once('/')?;
            (krate.to_owned(), rest)
        }
        None => ("north".to_owned(), path),
    };
    if rest.starts_with("src/") {
        return Some(Target { package, filter: "kind(lib)".into() });
    }
    let inner = rest.strip_prefix("tests/")?;
    let mut parts: Vec<&str> = inner.trim_end_matches(".rs").split('/').collect();
    let binary = parts.remove(0).to_owned();
    if parts.is_empty() {
        return Some(Target { package, filter: format!("binary({binary})") });
    }
    let module: Vec<&str> = parts.into_iter().filter(|part| *part != "mod" && *part != "main").collect();
    let filter = if module.is_empty() { format!("binary({binary})") } else { format!("binary({binary}) & test(/^{}::/)", module.join("::")) };
    Some(Target { package, filter })
}

/// Draft tasks from the worker ledger: first attempts reported Done and landed, whose landing changed test files.
pub fn candidates(ledger: &Path, repo: &Path, since: &str, until: &str, briefs: &Path) -> Result<Vec<Task>, NorthError> {
    let db = rusqlite::Connection::open_with_flags(ledger, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| refusal(format!("{}: {error}", ledger.display())))?;
    let mut statement = db
        .prepare(
            "SELECT agent, item, landed_sha, category, COALESCE(spec, ''), model, effort, brief_text FROM runs
             WHERE outcome = 'done' AND follows IS NULL AND landed_sha IS NOT NULL AND brief_text IS NOT NULL
               AND item LIKE 'north#%' AND ended >= ?1 AND ended < ?2 ORDER BY ended",
        )
        .map_err(|error| refusal(error.to_string()))?;
    let rows = statement
        .query_map(params![since, until], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<String>>(6)?, row.get::<_, String>(7)?))
        })
        .map_err(|error| refusal(error.to_string()))?;
    fs::create_dir_all(briefs).map_err(NorthError::Io)?;
    let mut seen = BTreeSet::new();
    let mut tasks = Vec::new();
    for row in rows.flatten() {
        let (agent, item, landed, category, spec, model, effort, brief) = row;
        let (Some(model), Some(effort)) = (model, effort) else { continue };
        if !seen.insert(landed.clone()) {
            continue;
        }
        let Ok(base) = super::sandbox::git_text(repo, &["rev-parse", &format!("{landed}^")]) else { continue };
        let changed = super::sandbox::git_text(repo, &["show", "--name-only", "--format=", &landed]).unwrap_or_default();
        let hidden: Vec<String> = changed.lines().filter(|path| is_test_file(path)).map(str::to_owned).collect();
        let targets: BTreeSet<Target> = hidden.iter().filter_map(|path| target_of(path)).collect();
        if targets.is_empty() {
            continue;
        }
        let id = format!("{}-{}", item.replace('#', "-"), &landed[..10]);
        fs::write(briefs.join(format!("{id}.md")), &brief).map_err(NorthError::Io)?;
        tasks.push(Task { id, repo: "north".into(), issue: item, run: agent, base, landed, category, spec, model, effort, brief_sha256: sha256(&brief), hidden, targets: targets.into_iter().collect() });
    }
    Ok(tasks)
}
