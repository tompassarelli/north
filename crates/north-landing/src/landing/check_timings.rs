use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Timing {
    Finished(u64),
    Exceeded(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Base {
    Within(u64),
    Exceeded,
    Untimed,
}

pub struct Timings {
    root: PathBuf,
}

impl Timings {
    pub fn at(root: PathBuf) -> Self {
        Timings { root }
    }

    pub fn default_root() -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        state.join("safe-push/check-timings")
    }

    fn file(&self, tree: &str, command: &str, class: &str) -> io::Result<PathBuf> {
        if tree.is_empty() || !tree.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a check timing names a tree by its hex object id"));
        }
        let mut hasher = Sha256::new();
        for part in [command, class] {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
        let digest: String = hasher.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect();
        Ok(self.root.join(tree).join(format!("{digest}.tsv")))
    }

    pub fn record(&self, tree: &str, command: &str, class: &str, timing: Timing) -> io::Result<()> {
        let path = self.file(tree, command, class)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let line = match timing {
            Timing::Finished(seconds) => format!("finished\t{seconds}\n"),
            Timing::Exceeded(seconds) => format!("exceeded\t{seconds}\n"),
        };
        OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
    }

    pub fn base(&self, tree: &str, command: &str, class: &str, deadline: u64) -> Base {
        let text = self.file(tree, command, class).and_then(fs::read_to_string).unwrap_or_default();
        let timings: Vec<Timing> = text
            .lines()
            .filter_map(|line| {
                let (kind, seconds) = line.split_once('\t')?;
                let seconds = seconds.trim().parse().ok()?;
                match kind {
                    "finished" => Some(Timing::Finished(seconds)),
                    "exceeded" => Some(Timing::Exceeded(seconds)),
                    _ => None,
                }
            })
            .collect();
        if timings.is_empty() {
            return Base::Untimed;
        }
        let mut within = None;
        for timing in timings {
            match timing {
                Timing::Finished(seconds) if seconds <= deadline => within = within.max(Some(seconds)),
                _ => return Base::Exceeded,
            }
        }
        within.map_or(Base::Exceeded, Base::Within)
    }
}

#[cfg(test)]
#[path = "check_timings_tests.rs"]
mod tests;
