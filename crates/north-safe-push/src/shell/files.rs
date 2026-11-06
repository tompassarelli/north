use std::path::{Path, PathBuf};

use north_base::platform::push_ledger;

pub struct Pushed<'a> {
    pub at_secs: u64,
    pub origin_url: &'a str,
    pub branch: &'a str,
    pub oid: &'a str,
    pub session: &'a str,
}

pub trait StateFiles {
    fn record_push(&self, pushed: &Pushed) -> std::io::Result<bool>;
    fn is_directory(&self, path: &Path) -> bool;
}

pub struct Live {
    state_home: PathBuf,
}

impl Live {
    pub fn new(state_home: &Path) -> Self {
        Self { state_home: state_home.to_path_buf() }
    }
}

impl StateFiles for Live {
    fn record_push(&self, pushed: &Pushed) -> std::io::Result<bool> {
        let repository = push_ledger::repository(pushed.origin_url);
        let push =
            push_ledger::Push { at_secs: pushed.at_secs, repository: &repository, branch: pushed.branch, oid: pushed.oid, landed: "", session: pushed.session };
        push_ledger::record(&self.state_home, &push)
    }

    fn is_directory(&self, path: &Path) -> bool {
        path.is_dir()
    }
}
