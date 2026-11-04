use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::{Launch, LaunchError, Moved, Placement, Source, World};
use crate::ops::timers::{child_failure, env_or, home, north_program, text};

pub(super) struct System {
    pub toml: PathBuf,
    cache: PathBuf,
    south: PathBuf,
}

impl System {
    pub fn from_env() -> Self {
        let home = home();
        Self {
            toml: env_or("WATCH_TOML", || home.join("code/south/main/agents/watch.toml")),
            cache: env_or("WATCH_CACHE", || env_or("XDG_CACHE_HOME", || home.join(".cache")).join("watch-review")),
            south: env_or("WATCH_SOUTH", || home.join("code/south/main")),
        }
    }

    #[cfg(test)]
    pub fn at(toml: PathBuf, cache: PathBuf) -> Self {
        Self { toml, cache, south: PathBuf::new() }
    }
}

fn run(command: &mut Command, name: &str) -> Result<Output, String> {
    let output = command.output().map_err(|error| format!("{name}: {error}"))?;
    if output.status.success() { Ok(output) } else { Err(child_failure(name, output.status.code(), &output.stderr)) }
}

fn git(directory: &Path, args: &[&str]) -> Result<String, String> {
    run(Command::new("git").arg("-C").arg(directory).args(args), "git").map(|output| text(&output).trim().to_owned())
}

pub(super) fn held(reason: &str) -> bool {
    ["exhausted", "saturated", "no Claude account", "held:"].iter().any(|word| reason.contains(word))
}

pub(super) fn placement(output: &str) -> Option<Placement> {
    let target = output.lines().find_map(|line| line.split_once(": target ").map(|(_, rest)| rest).or_else(|| line.strip_prefix("target ")))?;
    let first = target.split_whitespace().next().unwrap_or_default();
    let account = target.split_whitespace().nth(1).and_then(|word| word.strip_prefix("claude:"));
    Some(match account {
        Some(account) if !first.ends_with(':') && first != "anthropic-cloud" => Placement::Account(account.to_owned()),
        _ => Placement::Held(target.to_owned()),
    })
}

impl World for System {
    fn moved(&self, source: &Source) -> Result<Moved, String> {
        fs::create_dir_all(&self.cache).map_err(|error| format!("{}: {error}", self.cache.display()))?;
        let clone = self.cache.join(format!("{}.git", source.id));
        if clone.is_dir() {
            git(&clone, &["remote", "set-url", "origin", &source.repo])?;
            git(&clone, &["fetch", "--quiet", "origin", "+HEAD:refs/watch/head"]).map_err(|why| format!("fetch failed: {why}"))?;
        } else {
            let target = clone.to_string_lossy().into_owned();
            run(Command::new("git").args(["clone", "--quiet", "--bare", "--filter=blob:none", "--", &source.repo, &target]), "git").map_err(|why| format!("clone failed: {why}"))?;
            git(&clone, &["update-ref", "refs/watch/head", "HEAD"])?;
        }
        git(&clone, &["rev-parse", "--verify", "--quiet", &format!("{}^{{commit}}", source.last_commit)]).map_err(|_| format!("{} not found", source.last_commit))?;
        let range = format!("{}..refs/watch/head", source.last_commit);
        let mut args = vec!["rev-list", "--count", range.as_str()];
        if !source.path.is_empty() {
            args.extend(["--", source.path.as_str()]);
        }
        let count = git(&clone, &args)?.parse().map_err(|_| "rev-list gave no count".to_owned())?;
        Ok(Moved { count, head: git(&clone, &["rev-parse", "--short", "refs/watch/head"])? })
    }

    fn sessions(&self) -> Result<Vec<String>, String> {
        let listed = run(Command::new(north_program()).args(["fleet", "list"]), "north fleet list")?;
        Ok(text(&listed).lines().map(str::to_owned).collect())
    }

    fn place(&self, brief: &str) -> Result<Placement, String> {
        let file = tempfile::Builder::new().prefix("watch-review-brief").suffix(".md").tempfile().map_err(|error| error.to_string())?;
        fs::write(file.path(), brief).map_err(|error| error.to_string())?;
        let output = Command::new(north_program()).args(["agents", "plan", "--place"]).arg(file.path()).output().map_err(|error| format!("north agents plan --place: {error}"))?;
        let combined = format!("{}\n{}", text(&output), String::from_utf8_lossy(&output.stderr));
        placement(&combined).ok_or_else(|| child_failure("north agents plan --place", output.status.code(), &output.stderr))
    }

    fn worktree(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.south.parent().unwrap_or(&self.south).join("worktrees").join(name);
        git(&self.south, &["fetch", "--quiet", "origin"])?;
        git(&self.south, &["worktree", "add", "--quiet", "-b", name, &path.to_string_lossy(), "origin/main"])?;
        Ok(path)
    }

    fn discard(&self, name: &str, path: &Path) {
        let _ = git(&self.south, &["worktree", "remove", "--force", &path.to_string_lossy()]);
        let _ = git(&self.south, &["branch", "-D", name]);
    }

    fn launch(&self, launch: &Launch) -> Result<(), LaunchError> {
        let output = Command::new(north_program())
            .args(["fleet", "launch", "--provider", "claude", "--account", launch.account, "--parent", "none", "--name", launch.name, "--cwd"])
            .arg(launch.cwd)
            .arg(launch.brief)
            .output()
            .map_err(|error| LaunchError::Failed(format!("north fleet launch: {error}")))?;
        if output.status.success() {
            return Ok(());
        }
        let reason = child_failure("north fleet launch", output.status.code(), &output.stderr);
        Err(if held(&reason) { LaunchError::Held(reason) } else { LaunchError::Failed(reason) })
    }

    fn tell_proxy(&self, text: &str) -> Result<(), String> {
        run(Command::new(north_program()).args(["fleet", "send", "proxy"]).arg(text), "north fleet send proxy").map(drop)
    }
}
