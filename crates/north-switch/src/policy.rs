//! What `north policy off` takes away on this host and how it is recorded. The surfaces are north's links and
//! directories that hand agent sessions their instructions, skills and hook scripts; the settings keys are the
//! hooks and the worker-sandbox shell prefix north projects into `~/.claude/settings.json`, which every account
//! profile links; the checks are the git hooks of each checkout under `~/code`. The record in
//! `$XDG_STATE_HOME/north/policy-off/record` is also the marker: while it exists, `north hook` answers nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SURFACES: [&str; 9] = [".claude/CLAUDE.md", ".claude/skills", ".claude/agents", ".agents/AGENTS.md", ".agents/skills", ".agents/hooks", ".codex/AGENTS.md", ".codex/skills", "code/AGENTS.md"];
pub const SETTINGS: &str = ".claude/settings.json";
pub const SHELL_PREFIX: &str = "CLAUDE_CODE_SHELL_PREFIX";
pub const CHECKOUTS: &str = "code";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Off,
    Moved(String),
    Settings,
    Hooks { repository: PathBuf, before: Option<String> },
}

impl Entry {
    fn line(&self) -> String {
        match self {
            Self::Off => "off".to_owned(),
            Self::Moved(path) => format!("moved\t{path}"),
            Self::Settings => "settings".to_owned(),
            Self::Hooks { repository, before } => format!("hooks\t{}\t{}", repository.display(), before.as_deref().unwrap_or("-")),
        }
    }

    fn parse(line: &str) -> Option<Self> {
        match line.split('\t').collect::<Vec<_>>().as_slice() {
            ["moved", path] if SURFACES.contains(path) => Some(Self::Moved((*path).to_owned())),
            ["off"] => Some(Self::Off),
            ["settings"] => Some(Self::Settings),
            ["hooks", repository, before] => Some(Self::Hooks { repository: PathBuf::from(repository), before: (*before != "-").then(|| (*before).to_owned()) }),
            _ => None,
        }
    }
}

pub struct Stash {
    pub root: PathBuf,
}

impl Stash {
    pub fn new(state_home: &Path) -> Self {
        Self { root: state_home.join("north/policy-off") }
    }

    pub fn record(&self) -> PathBuf {
        self.root.join("record")
    }

    pub fn moved(&self, surface: &str) -> PathBuf {
        self.root.join("moved").join(surface)
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn no_hooks(&self) -> PathBuf {
        self.root.join("no-hooks")
    }

    pub fn is_off(&self) -> bool {
        self.record().exists()
    }

    pub fn read(&self) -> Result<Vec<Entry>, String> {
        let path = self.record();
        match fs::read_to_string(&path) {
            Ok(text) => text.lines().map(|line| Entry::parse(line).ok_or_else(|| format!("{}: not a line north policy off writes: {line}", path.display()))).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    pub fn write(&self, entries: &[Entry]) -> Result<(), String> {
        let path = self.record();
        if entries.is_empty() {
            return match fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {error}", path.display())),
                _ => Ok(()),
            };
        }
        let text: String = entries.iter().map(|entry| entry.line() + "\n").collect();
        let staged = path.with_extension("new");
        fs::create_dir_all(&self.root).and_then(|()| fs::write(&staged, text)).and_then(|()| fs::rename(&staged, &path)).map_err(|error| format!("{}: {error}", path.display()))
    }
}

pub fn checkouts(home: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(home.join(CHECKOUTS)).into_iter().flatten().flatten().map(|project| project.path().join("main")).filter(|main| is_work_tree(main)).collect();
    found.sort();
    found
}

fn is_work_tree(directory: &Path) -> bool {
    directory.is_dir() && Command::new("git").arg("-C").arg(directory).args(["rev-parse", "--is-inside-work-tree"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}
