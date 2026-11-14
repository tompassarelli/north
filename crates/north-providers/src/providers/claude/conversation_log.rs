use std::fs;
use std::path::PathBuf;

use crate::fleet::model::Account;

const PROJECTS: &str = "projects";

pub(super) fn find(accounts: &[Account], conversation: &str) -> Option<PathBuf> {
    let file = format!("{conversation}.jsonl");
    accounts
        .iter()
        .flat_map(|account| fs::read_dir(account.home.join(PROJECTS)).into_iter().flatten().flatten())
        .map(|project| project.path().join(&file))
        .find(|path| path.is_file())
}
