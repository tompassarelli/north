use std::fs;
use std::path::{Path, PathBuf};

use north_base::error::NorthError;
use serde::{Deserialize, Serialize};

use super::oracle::{self, Oracle};
use super::sandbox::{Sandbox, git_text};
use super::task::Task;

/// One arm's grade: every fail-to-pass test passes, and no test that passes at the landing fails.
/// A pass-to-pass test the arm deleted is absent, not failed, and does not count against it.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Grade {
    pub task: String,
    pub correct: bool,
    pub tree: String,
    pub fail_to_pass: usize,
    pub fail_to_pass_passed: usize,
    pub broken: Vec<String>,
    pub leaks: Vec<String>,
}

/// The tree an arm left, as the arm sees it: what it pushed to its origin (cloned afresh), else the
/// lane it changed, else the main clone.
fn final_tree(sandbox: &Sandbox, task: &Task) -> Result<PathBuf, NorthError> {
    let graded = sandbox.root.join("graded");
    if graded.exists() {
        return Ok(graded);
    }
    if let Some(tip) = sandbox.pushed(&task.base) {
        git_text(&sandbox.root, &["clone", "--quiet", &sandbox.origin.display().to_string(), "graded"])?;
        git_text(&graded, &["checkout", "--quiet", "--detach", &tip])?;
        return Ok(graded);
    }
    let main = sandbox.inside();
    let trees = sandbox.worktrees();
    Ok(trees.iter().filter(|tree| **tree != main).find(|tree| sandbox.touched(tree, &task.base)).cloned().unwrap_or(main))
}

/// The same directory as seen from outside the arm's view, for plain file writes.
fn outside(sandbox: &Sandbox, tree: &Path) -> PathBuf {
    tree.strip_prefix(super::home().join("code")).map_or_else(|_| tree.to_owned(), |rest| sandbox.code.join(rest))
}

fn leaks(sandbox: &Sandbox, task: &Task) -> Vec<String> {
    let stream = fs::read_to_string(sandbox.root.join("stream.jsonl")).unwrap_or_default();
    reached(&stream, &task.landed)
}

/// Twelve characters of the sha: a task id carries ten of them, and the id appears in every path the arm prints.
pub(crate) fn reached(stream: &str, landed: &str) -> Vec<String> {
    [(&landed[..12], "the landed sha"), ("github.com/tompassarelli", "the public repository"), ("api.github.com", "the GitHub API")].into_iter().filter(|(needle, _)| stream.contains(needle)).map(|(_, what)| what.to_owned()).collect()
}

pub fn grade(task: &Task, sandbox: &Sandbox, source: &Path, oracle: &Oracle) -> Result<Grade, NorthError> {
    let tree = final_tree(sandbox, task)?;
    oracle::overlay(&outside(sandbox, &tree), source, task)?;
    let verdicts = oracle::run_tests(&sandbox.view(), &tree, task, &sandbox.root.join("grade.log"), None)?;
    let fail_to_pass_passed = oracle.fail_to_pass.iter().filter(|id| verdicts.get(*id).copied().unwrap_or(false)).count();
    let broken: Vec<String> = oracle.reference.iter().filter(|id| verdicts.get(*id) == Some(&false)).cloned().collect();
    Ok(Grade {
        task: task.id.clone(),
        correct: !oracle.fail_to_pass.is_empty() && fail_to_pass_passed == oracle.fail_to_pass.len() && broken.is_empty(),
        tree: tree.display().to_string(),
        fail_to_pass: oracle.fail_to_pass.len(),
        fail_to_pass_passed,
        broken,
        leaks: leaks(sandbox, task),
    })
}
