use std::path::PathBuf;

use crate::fleet::registry::rewritten::Rewrites;
use crate::foundation::stale_rewrites;

pub(super) struct Recorded;

impl Rewrites for Recorded {
    fn rewrites(&self) -> Vec<(String, String)> {
        stale_rewrites::read(&stale_rewrites::file(&PathBuf::from(std::env::var_os("HOME").unwrap_or_default()))).unwrap_or_default()
    }
}
