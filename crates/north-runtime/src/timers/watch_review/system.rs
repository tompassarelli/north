use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::fleet_session::{Fleet, Launch, LaunchError, Placement, git, run};
use super::{Moved, Source, World};
use crate::timers::{env_or, home};

pub(super) struct System {
    pub toml: PathBuf,
    cache: PathBuf,
    fleet: Fleet,
}

impl System {
    pub fn from_env() -> Self {
        let home = home();
        Self {
            toml: env_or("WATCH_TOML", || home.join("code/south/main/agents/watch.toml")),
            cache: env_or("WATCH_CACHE", || env_or("XDG_CACHE_HOME", || home.join(".cache")).join("watch-review")),
            fleet: Fleet { south: env_or("WATCH_SOUTH", || home.join("code/south/main")), home },
        }
    }
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
        let count = content_commits(&clone, &source.last_commit, &source.path)?;
        Ok(Moved { count, head: git(&clone, &["rev-parse", "--short", "refs/watch/head"])? })
    }

    fn sessions(&self) -> Result<Vec<String>, String> {
        self.fleet.sessions()
    }

    fn place(&self, brief: &str) -> Result<Placement, String> {
        self.fleet.place(brief)
    }

    fn worktree(&self, name: &str) -> Result<PathBuf, String> {
        self.fleet.worktree(name)
    }

    fn discard(&self, name: &str, path: &Path) {
        self.fleet.discard(name, path);
    }

    fn launch(&self, launch: &Launch) -> Result<(), LaunchError> {
        self.fleet.launch(launch)
    }

    fn tell_proxy(&self, text: &str) -> Result<(), String> {
        self.fleet.tell_proxy(text)
    }
}

fn content_commits(clone: &Path, last_commit: &str, path: &str) -> Result<usize, String> {
    let range = format!("{last_commit}..refs/watch/head");
    let mut args = vec!["rev-list", "--count", "--no-merges", range.as_str()];
    if !path.is_empty() {
        args.extend(["--", path]);
    }
    git(clone, &args)?.parse().map_err(|_| "rev-list gave no count".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_carrying_two_branches_changes_across_counts_as_no_new_commit() {
        let repo = tempfile::tempdir().unwrap();
        let dir = repo.path();
        let commit = |file: &str, text: &str| {
            fs::write(dir.join(file), text).unwrap();
            git(dir, &["add", file]).unwrap();
            git(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", text]).unwrap();
        };
        git(dir, &["init", "-q", "-b", "main"]).unwrap();
        commit("notes.md", "base\n");
        let base = git(dir, &["rev-parse", "HEAD"]).unwrap();
        git(dir, &["checkout", "-q", "-b", "staging"]).unwrap();
        commit("notes.md", "base\nstaging\n");
        git(dir, &["checkout", "-q", "main"]).unwrap();
        commit("other.md", "main\n");
        commit("notes.md", "main\nbase\n");
        git(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "merge", "-q", "--no-edit", "staging"]).unwrap();
        git(dir, &["update-ref", "refs/watch/head", "HEAD"]).unwrap();
        assert_eq!(content_commits(dir, &base, "notes.md"), Ok(2));
    }
}
