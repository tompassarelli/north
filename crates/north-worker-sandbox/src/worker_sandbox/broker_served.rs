//! What one broker serves beyond the host's own projects, as the words after `north --worker-sandbox --broker` say it: the projects under one more directory.

use std::ffi::OsString;
use std::path::PathBuf;

use super::CODE;

/// What a broker serves beyond the host's own projects: those under one more directory.
#[derive(Clone, Default)]
pub(super) struct Served {
    pub code: Option<PathBuf>,
}

impl Served {
    /// The words after `--broker` that start a broker serving this.
    pub(super) fn words(&self) -> Vec<OsString> {
        self.code.iter().flat_map(|code| [OsString::from(CODE), code.clone().into_os_string()]).collect()
    }
}
