use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::fleet_session::{Fleet, Launch, LaunchError, Placement, git, run};
use super::{Moved, Source, World};
use crate::ops::timers::{env_or, home};

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
            fleet: Fleet { south: env_or("WATCH_SOUTH", || home.join("code/south/main")) },
        }
    }

    #[cfg(test)]
    pub fn at(toml: PathBuf, cache: PathBuf) -> Self {
        Self { toml, cache, fleet: Fleet { south: PathBuf::new() } }
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
        let range = format!("{}..refs/watch/head", source.last_commit);
        let mut args = vec!["rev-list", "--count", range.as_str()];
        if !source.path.is_empty() {
            args.extend(["--", source.path.as_str()]);
        }
        let count = git(&clone, &args)?.parse().map_err(|_| "rev-list gave no count".to_owned())?;
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
