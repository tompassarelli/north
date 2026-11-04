use std::path::Path;
use std::process::Command;

const SEPARATOR: &str = " :: ";
const REF_SHORT: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub workflow: String,
    pub publish: Option<String>,
    pub runner: Option<String>,
    checks: Vec<String>,
    local_only: Vec<String>,
}

fn git_lines(repo: &Path, arguments: &[&str]) -> Vec<String> {
    let Ok(output) = Command::new("git").arg("-C").arg(repo).args(arguments).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

fn blob_values(repo: &Path, head: &str, key: &str) -> Vec<String> {
    let blob = format!("{head}:.safe-push");
    git_lines(repo, &["config", "--blob", &blob, "--get-all", key])
}

impl Config {
    pub fn load(repo: &Path, head: &str) -> Self {
        let first = |key: &str| blob_values(repo, head, key).into_iter().next_back();
        Self {
            workflow: first("land.workflow").unwrap_or_default(),
            publish: first("land.publish").filter(|prefix| !prefix.is_empty()),
            runner: first("land.runner"),
            checks: blob_values(repo, head, "land.check"),
            local_only: blob_values(repo, head, "land.local-only"),
        }
    }

    pub fn has_checks(&self) -> bool {
        !self.checks.is_empty()
    }

    pub fn local_reason(&self, command: &str) -> Option<&str> {
        let suffix = format!("{SEPARATOR}{command}");
        self.local_only.iter().find(|entry| entry.ends_with(&suffix)).map(|entry| entry.split(SEPARATOR).next().unwrap_or_default())
    }

    pub fn publish_ref(&self, head: &str) -> Option<String> {
        self.publish.as_ref().map(|prefix| format!("{prefix}{}", &head[..head.len().min(REF_SHORT)]))
    }

    pub fn commands(&self, repo: &Path, base: &str, head: &str) -> Result<Vec<String>, String> {
        let mut commands: Vec<String> = Vec::new();
        for entry in &self.checks {
            let Some((patterns, command)) = entry.split_once(SEPARATOR) else {
                return Err(format!("safe-push: invalid land.check: {entry}; NOT pushed."));
            };
            if patterns.is_empty() || command.is_empty() {
                return Err(format!("safe-push: invalid land.check: {entry}; NOT pushed."));
            }
            if commands.iter().any(|seen| seen == command) || !touches(repo, base, head, patterns) {
                continue;
            }
            commands.push(command.to_owned());
        }
        Ok(commands)
    }
}

fn touches(repo: &Path, base: &str, head: &str, patterns: &str) -> bool {
    let specs: Vec<&str> = patterns.split(',').collect();
    let mut arguments: Vec<&str> = if base.is_empty() {
        vec!["ls-tree", "-r", "--name-only", head, "--"]
    } else {
        vec!["diff", "--name-only", "--no-renames", base, head, "--"]
    };
    arguments.extend(specs);
    git_lines(repo, &arguments).iter().any(|line| !line.is_empty())
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
