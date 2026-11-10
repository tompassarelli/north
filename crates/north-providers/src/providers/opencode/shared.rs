use std::io;
use std::path::{Path, PathBuf};

use super::route::Route;

const POLICY: &str = "instructions/shared/AGENTS.md";
const SKILLS: &str = "skills/shared";

/// The current published agent generation, the one source of Tom's policy text and skills that Claude and Codex read through their home directories.
#[derive(Clone, Debug)]
pub struct Shared {
    generation: PathBuf,
}

impl Shared {
    pub fn published() -> Option<Self> {
        let root = std::env::var_os("NORTH_AGENT_STATE_ROOT").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/north/agents")))?;
        Some(Self::at(&root))
    }

    pub fn at(root: &Path) -> Self {
        Self { generation: root.join("current") }
    }

    pub fn policy(&self) -> Option<String> {
        std::fs::read_to_string(self.generation.join(POLICY)).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
    }

    pub fn heading(&self) -> Option<String> {
        self.policy()?.lines().find_map(|line| line.strip_prefix("# ").map(|heading| heading.trim().to_owned()))
    }

    /// Copies the shared skills under `into`, where a worker confined to its lane can read them; the generation itself sits outside what its sandbox admits.
    pub fn project_skills(&self, into: &Path) -> io::Result<()> {
        let source = self.generation.join(SKILLS);
        if source.is_dir() {
            copy_tree(&source, into)?;
        }
        Ok(())
    }
}

/// The first-turn text of a worker on `route`: `delivered` with the shared policy after it, and the shared skills projected under `skills`; the free route gets neither, because it may train on its prompts and the shared text is private.
pub fn deliver(shared: Option<&Shared>, route: Route, delivered: String, skills: &Path) -> io::Result<String> {
    let Some(shared) = shared.filter(|_| route == Route::Go) else { return Ok(delivered) };
    shared.project_skills(skills)?;
    Ok(shared.policy().map_or(delivered.clone(), |policy| format!("{delivered}\n\n{policy}")))
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if std::fs::metadata(entry.path())?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
