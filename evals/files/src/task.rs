use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::proc::git;

pub struct Task {
    pub id: String,
    pub repo: PathBuf,
    pub base: String,
    pub text: String,
    pub references: Vec<String>,
}

pub struct Tree {
    pub sizes: BTreeMap<String, u64>,
}

pub fn load(path: &Path) -> Result<Vec<Task>, String> {
    let json: Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())?;
    let field = |task: &Value, key: &str| task[key].as_str().map(str::to_owned).ok_or_else(|| format!("task without {key}"));
    json["task"].as_array().ok_or("no task array")?.iter().map(|task| {
        Ok(Task {
            id: field(task, "id")?,
            repo: PathBuf::from(field(task, "repo_path")?),
            base: field(task, "base")?,
            text: field(task, "text")?,
            references: task["reference"].as_array().ok_or("no reference")?.iter().filter_map(|r| r.as_str().map(str::to_owned)).collect(),
        })
    }).collect()
}

impl Tree {
    pub fn at(repo: &Path, base: &str) -> Result<Tree, String> {
        let listed = git(repo, &["ls-tree", "-r", "-l", "-z", base], None)?.text();
        let sizes = listed.split('\0').filter_map(|entry| {
            let (meta, path) = entry.split_once('\t')?;
            let size = meta.split_whitespace().nth(3)?.parse().ok()?;
            Some((path.to_owned(), size))
        }).collect();
        Ok(Tree { sizes })
    }

    pub fn has(&self, path: &str) -> bool {
        self.sizes.contains_key(path)
    }
}

pub fn commit_time(repo: &Path, base: &str) -> Result<String, String> {
    Ok(git(repo, &["show", "-s", "--format=%ct", base], None)?.text().trim().to_owned())
}

pub fn truth(task: &Task, wisp: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    for reference in &task.references {
        let (repo, commit, prefix) = match reference.strip_prefix("wisp:") {
            Some(commit) => (wisp, commit, "wisp:"),
            None => (task.repo.as_path(), reference.as_str(), ""),
        };
        let parent = format!("{commit}^");
        let changed = git(repo, &["diff", "--name-only", &parent, commit], None)?.text();
        files.extend(changed.lines().map(|line| format!("{prefix}{line}")));
    }
    Ok(files)
}
