use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::fleet_session::{Fleet, Launch, LaunchError, Placement, run};
use super::World;
use crate::timers::{env_or, home, north_program};

pub(super) struct System {
    pub state: PathBuf,
    fleet: Fleet,
}

impl System {
    pub fn from_env() -> Self {
        let home = home();
        Self {
            state: env_or("INCIDENT_RETRO_STATE", || home.join(".local/state/agents/incident-retro")),
            fleet: Fleet { south: env_or("INCIDENT_RETRO_SOUTH", || home.join("code/south/main")) },
        }
    }
}

pub(super) fn chunk_number(path: &Path) -> Option<usize> {
    path.file_name()?.to_str()?.strip_prefix("chunk-")?.strip_suffix(".jsonl")?.parse().ok()
}

impl World for System {
    fn mine(&self, out: &Path) -> Result<Vec<String>, String> {
        run(Command::new(north_program()).args(["incident-mine", "--days", "7", "--chunks", "6", "--per-thread", "3", "--out"]).arg(out), "north incident-mine")?;
        let mut chunks: Vec<(usize, PathBuf)> = fs::read_dir(out)
            .map_err(|error| format!("{}: {error}", out.display()))?
            .flatten()
            .filter_map(|entry| chunk_number(&entry.path()).map(|number| (number, entry.path())))
            .collect();
        chunks.sort();
        let mut lines = Vec::new();
        for (_, path) in chunks {
            let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            lines.extend(text.lines().filter(|line| !line.trim().is_empty()).map(str::to_owned));
        }
        Ok(lines)
    }

    fn save(&self, path: &Path, text: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
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
