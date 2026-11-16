use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub type AccountHome = fn(&str, &str) -> Option<PathBuf>;
pub type StatusVerb = fn(&[&str]) -> bool;
pub type FileToolRefusal = fn(&[String], &Path) -> Option<String>;
pub type FileReadRefusal = fn(&[String], &Path) -> Option<String>;

#[derive(Clone, Copy)]
pub struct Host {
    pub account_home: AccountHome,
    pub status_verb: StatusVerb,
    pub file_tool_refusal: FileToolRefusal,
    pub file_read_refusal: FileReadRefusal,
}

static INSTALLED: OnceLock<Host> = OnceLock::new();

pub fn install(host: Host) {
    let _ = INSTALLED.set(host);
}

fn installed() -> Result<Host, String> {
    INSTALLED.get().copied().ok_or_else(|| "north hook: no guard host installed".to_owned())
}

pub fn account_home(provider: &str, label: &str) -> Option<PathBuf> {
    installed().ok().and_then(|host| (host.account_home)(provider, label))
}

pub fn status_verb(words: &[&str]) -> bool {
    installed().is_ok_and(|host| (host.status_verb)(words))
}

/// The first of `paths` a file tool of this session may not write, where this host confines its shell; `None` where no host is installed, which only a test is.
pub fn file_tool_refusal(paths: &[String], cwd: &Path) -> Option<String> {
    installed().ok().and_then(|host| (host.file_tool_refusal)(paths, cwd))
}

/// The first of `paths` a reading tool of this session may not reach, where this host confines its shell; `None` where no host is installed, which only a test is.
pub fn file_read_refusal(paths: &[String], cwd: &Path) -> Option<String> {
    installed().ok().and_then(|host| (host.file_read_refusal)(paths, cwd))
}
