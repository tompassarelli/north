use std::path::{Path, PathBuf};

use super::super::fleet_session::{Fleet, Launch, LaunchError, Placement};
use super::World;
use crate::timers::{env_or, home};

pub(super) struct System {
    pub toml: PathBuf,
    fleet: Fleet,
}

impl System {
    pub fn from_env() -> Self {
        let home = home();
        Self { toml: env_or("WATCH_TOML", || home.join("code/south/main/agents/watch.toml")), fleet: Fleet { south: env_or("WATCH_SOUTH", || home.join("code/south/main")), home } }
    }
}

impl World for System {
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
