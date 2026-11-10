use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::merge_set::{self, Kind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Rebase,
    CherryPick,
}

impl Operation {
    fn verb(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
        }
    }
}

fn git(worktree: &Path, arguments: &[&str]) -> Result<Output, String> {
    Command::new("git").arg("-C").arg(worktree).args(merge_set::driver_arguments()).args(arguments).env("GIT_EDITOR", "true").output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

fn stdout(worktree: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = git(worktree, arguments)?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_path(worktree: &Path, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(stdout(worktree, &["rev-parse", "--git-path", name])?.trim());
    Ok(if path.is_absolute() { path } else { worktree.join(path) })
}

fn in_progress(worktree: &Path) -> Result<Option<Operation>, String> {
    for (name, operation) in [("rebase-merge", Operation::Rebase), ("rebase-apply", Operation::Rebase), ("CHERRY_PICK_HEAD", Operation::CherryPick)] {
        if git_path(worktree, name)?.exists() {
            return Ok(Some(operation));
        }
    }
    Ok(None)
}

fn generator(worktree: &Path, command: &str, code: &[String]) -> Result<Command, String> {
    let mut words = command.split_whitespace();
    let program = words.next().ok_or("an empty regenerate command")?;
    let head_north = worktree.join("bin/head-north");
    let mut command = match program {
        "north" if !code.is_empty() && head_north.is_file() => {
            let mut built = Command::new(head_north);
            built.args(["--code", &code.join(",")]);
            built
        }
        "north" => Command::new(std::env::current_exe().map_err(|error| format!("the running north: {error}"))?),
        other => Command::new(other),
    };
    command.args(words);
    Ok(command)
}

fn generate(worktree: &Path, command: &str, code: &[String]) -> Result<(), String> {
    let output = generator(worktree, command, code)?.current_dir(worktree).output().map_err(|error| format!("{command}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{command} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}

fn regenerate(worktree: &Path, rules: &[merge_set::Rule], operation: Operation) -> Result<(), String> {
    let conflicted = stdout(worktree, &["diff", "--name-only", "-z", "--diff-filter=U"])?;
    let conflicted = conflicted.split('\0').filter(|path| !path.is_empty()).collect::<Vec<_>>();
    if conflicted.is_empty() {
        return Err(format!("the {} stopped with no conflicted file", operation.verb()));
    }
    let mut commands = Vec::new();
    for path in &conflicted {
        match rules.iter().find(|rule| rule.path == *path).map(|rule| &rule.kind) {
            Some(Kind::Regenerate { command, generator, .. }) if !commands.iter().any(|(known, _)| *known == command) => commands.push((command, generator)),
            Some(Kind::Regenerate { .. }) => {}
            _ => return Err(format!("{path} conflicts and is not a regenerated file")),
        }
    }
    for (command, generator) in commands {
        generate(worktree, command, generator)?;
    }
    let mut add = vec!["add", "--"];
    add.extend(conflicted);
    stdout(worktree, &add).map(drop)
}

pub fn finish(worktree: &Path, config: &Path) -> Result<(), String> {
    let top = PathBuf::from(stdout(worktree, &["rev-parse", "--show-toplevel"])?.trim());
    let worktree = top.as_path();
    let rules = merge_set::read_rules(config)?;
    while let Some(operation) = in_progress(worktree)? {
        regenerate(worktree, &rules, operation)?;
        let before = stdout(worktree, &["rev-parse", "HEAD"])?;
        let continued = git(worktree, &[operation.verb(), "--continue"])?;
        let stopped_again = stdout(worktree, &["rev-parse", "HEAD"])? != before && in_progress(worktree)?.is_some();
        if !continued.status.success() && !stopped_again {
            return Err(format!("git {} --continue failed: {}", operation.verb(), String::from_utf8_lossy(&continued.stderr).trim()));
        }
    }
    Ok(())
}

fn names(worktree: &Path, range: &str) -> Result<Vec<String>, String> {
    Ok(stdout(worktree, &["diff", "--name-only", "-z", range])?.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

pub fn generator_moved(worktree: &Path, config: &Path, base: &str, picked_onto: &str) -> Result<Option<String>, String> {
    if !config.exists() || base == picked_onto {
        return Ok(None);
    }
    let rules = merge_set::read_rules(config)?;
    let (ahead, pick) = (names(worktree, &format!("{base}..{picked_onto}"))?, names(worktree, &format!("{picked_onto}..HEAD"))?);
    Ok(merge_set::generator_moved(&rules, &ahead, &pick))
}

pub fn fold(worktree: &Path, config: &Path, base: &str) -> Result<Vec<String>, String> {
    if !config.exists() || !git(worktree, &["diff", "--cached", "--quiet"])?.status.success() {
        return Ok(Vec::new());
    }
    let fork = stdout(worktree, &["merge-base", base, "HEAD"])?;
    let changed = stdout(worktree, &["diff", "--name-only", "-z", fork.trim(), "HEAD"])?;
    let changed: Vec<&str> = changed.split('\0').filter(|path| !path.is_empty()).collect();
    let mut folded = Vec::new();
    for rule in merge_set::read_rules(config)? {
        match &rule.kind {
            // The lander writes a generated file from its sources; the lane never commits one.
            Kind::Regenerate { .. } => continue,
            Kind::Block { start, end, sorted: true } if changed.contains(&rule.path.as_str()) => {
                let path = worktree.join(&rule.path);
                let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", rule.path))?;
                if let Some(sorted) = merge_set::sort_block(&text, start, end) {
                    std::fs::write(&path, sorted).map_err(|error| format!("{}: {error}", rule.path))?;
                }
            }
            _ => continue,
        }
        if !stdout(worktree, &["status", "--porcelain", "--", &rule.path])?.is_empty() {
            stdout(worktree, &["add", "--", &rule.path])?;
            folded.push(rule.path);
        }
    }
    if !folded.is_empty() {
        stdout(worktree, &["commit", "-q", "--amend", "--no-edit", "--no-verify"])?;
    }
    Ok(folded)
}

pub fn refresh(worktree: &Path, config: &Path, picked_onto: &str) -> Result<Vec<String>, String> {
    if !config.exists() {
        return Ok(Vec::new());
    }
    let rules = merge_set::read_rules(config)?;
    let changed = stdout(worktree, &["diff", "--name-only", "-z", picked_onto, "HEAD"])?;
    let changed = changed.split('\0').filter(|path| !path.is_empty()).collect::<Vec<_>>();
    let mut refreshed = Vec::new();
    for rule in &rules {
        let Kind::Regenerate { command, sources, generator } = &rule.kind else { continue };
        let stale = !changed.contains(&rule.path.as_str()) && changed.iter().any(|path| sources.iter().any(|source| path.starts_with(source.as_str())));
        if !stale {
            continue;
        }
        generate(worktree, command, generator)?;
        if !stdout(worktree, &["status", "--porcelain", "--", &rule.path])?.is_empty() {
            stdout(worktree, &["add", "--", &rule.path])?;
            refreshed.push(rule.path.clone());
        }
    }
    if !refreshed.is_empty() {
        stdout(worktree, &["commit", "-q", "--amend", "--no-edit", "--no-verify"])?;
    }
    Ok(refreshed)
}
