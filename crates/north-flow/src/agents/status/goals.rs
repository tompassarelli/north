use std::path::Path;

use crate::foundation::clock::Timestamp;

pub(super) fn lines(config: &Path, ledger: &Path) -> Vec<String> {
    match super::super::goal::read_config(config) {
        Ok(cfg) => super::super::goal::plan_lines(&cfg, ledger, Timestamp::now(), true),
        Err(error) => {
            eprintln!("agents plan: cannot run goal ETA: {error}");
            Vec::new()
        }
    }
}
