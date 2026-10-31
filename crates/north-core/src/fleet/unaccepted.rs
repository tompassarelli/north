use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "unaccepted.jsonl";
const REPLAYS: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Entry {
    id: String,
    line: String,
    replays: u32,
}

pub struct Unaccepted {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl Unaccepted {
    pub fn inherit(dir: &Path) -> Self {
        let mut entries = read(dir);
        for entry in &mut entries {
            entry.replays += 1;
        }
        entries.retain(|entry| entry.replays <= REPLAYS);
        let journal = Self { path: dir.join(FILE), entries };
        journal.save();
        journal
    }

    pub fn sent(&mut self, id: String, line: String) {
        if self.entries.iter().all(|entry| entry.id != id) {
            self.entries.push(Entry { id, line, replays: 0 });
            self.save();
        }
    }

    pub fn accepted(&mut self, id: &str) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() != before {
            self.save();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn to_replay(dir: &Path) -> Vec<String> {
        read(dir).into_iter().filter(|entry| entry.replays == REPLAYS).map(|entry| entry.line).collect()
    }

    pub fn discard(dir: &Path) {
        if let Err(error) = remove(&dir.join(FILE)) {
            eprintln!("north fleet: unaccepted prompts not discarded: {error}");
        }
    }

    pub fn outstanding(dir: &Path) -> bool {
        !read(dir).is_empty()
    }

    pub fn awaits_a_host(dir: &Path) -> bool {
        read(dir).iter().any(|entry| entry.replays < REPLAYS)
    }

    fn save(&self) {
        let outcome = if self.entries.is_empty() { remove(&self.path) } else { self.write() };
        if let Err(error) = outcome {
            eprintln!("north session-host: unaccepted prompts not saved: {error}");
        }
    }

    fn write(&self) -> std::io::Result<()> {
        let lines: Vec<String> = self.entries.iter().filter_map(|entry| serde_json::to_string(entry).ok()).collect();
        let staged = self.path.with_extension("tmp");
        fs::write(&staged, lines.join("\n") + "\n")?;
        fs::rename(staged, &self.path)
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn read(dir: &Path) -> Vec<Entry> {
    let text = fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_is_replayed_to_the_next_host_once_and_dropped_when_that_host_dies_with_it_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = Unaccepted::inherit(dir.path());
        first.sent("a".into(), "frame-a".into());
        first.sent("b".into(), "frame-b".into());
        first.accepted("a");
        assert!(Unaccepted::awaits_a_host(dir.path()));

        let mut second = Unaccepted::inherit(dir.path());
        assert_eq!(Unaccepted::to_replay(dir.path()), ["frame-b"]);
        assert!(!Unaccepted::awaits_a_host(dir.path()), "a replayed prompt does not call for a third host");
        second.sent("b".into(), "frame-b".into());
        assert_eq!(Unaccepted::to_replay(dir.path()), ["frame-b"], "writing the replayed frame again does not journal it twice");

        let third = Unaccepted::inherit(dir.path());
        assert!(third.is_empty() && Unaccepted::to_replay(dir.path()).is_empty());
        assert!(!dir.path().join(FILE).exists());
    }
}
