use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

use north_base::platform::push_ledger;

const SAFE_PUSH_DIRECTORY: &str = "safe-push";
const EVENTS: &str = "events.jsonl";
const STAGES: &str = "stages.tsv";

pub struct Pushed<'a> {
    pub at_secs: u64,
    pub origin_url: &'a str,
    pub branch: &'a str,
    pub oid: &'a str,
    pub session: &'a str,
}

pub enum Lock {
    Held(Box<dyn Send>),
    Busy,
    Unavailable,
}

pub trait StateFiles {
    fn record_push(&self, pushed: &Pushed) -> std::io::Result<bool>;
    fn is_directory(&self, path: &Path) -> bool;
    fn append_event(&self, line: &str) -> std::io::Result<()>;
    fn append_stage(&self, row: &str) -> std::io::Result<()>;
    fn output_logs(&self) -> Vec<PathBuf>;
    fn lock_exclusive(&self, path: &Path) -> Lock;
}

pub struct Live {
    state_home: PathBuf,
}

impl Live {
    pub fn new(state_home: &Path) -> Self {
        Self {
            state_home: state_home.to_path_buf(),
        }
    }

    fn append(&self, name: &str, text: &str) -> std::io::Result<()> {
        let directory = self.state_home.join(SAFE_PUSH_DIRECTORY);
        std::fs::create_dir_all(&directory)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join(name))?
            .write_all(text.as_bytes())
    }
}

impl StateFiles for Live {
    fn record_push(&self, pushed: &Pushed) -> std::io::Result<bool> {
        let repository = push_ledger::repository(pushed.origin_url);
        let push = push_ledger::Push {
            at_secs: pushed.at_secs,
            repository: &repository,
            branch: pushed.branch,
            oid: pushed.oid,
            landed: "",
            session: pushed.session,
        };
        push_ledger::record(&self.state_home, &push)
    }

    fn is_directory(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn append_event(&self, line: &str) -> std::io::Result<()> {
        self.append(EVENTS, line)
    }

    fn append_stage(&self, row: &str) -> std::io::Result<()> {
        self.append(STAGES, row)
    }

    fn output_logs(&self) -> Vec<PathBuf> {
        [1, 2]
            .iter()
            .filter_map(|fd| std::fs::read_link(format!("/proc/self/fd/{fd}")).ok())
            .filter(|target| target.is_file())
            .collect()
    }

    fn lock_exclusive(&self, path: &Path) -> Lock {
        let Ok(file) = File::open(path) else {
            return Lock::Unavailable;
        };
        match rustix::fs::flock(
            file.as_fd(),
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        ) {
            Ok(()) => Lock::Held(Box::new(file)),
            Err(_) => Lock::Busy,
        }
    }
}
