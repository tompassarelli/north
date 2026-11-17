use std::path::{Path, PathBuf};

use crate::providers::claude::token_quota;



pub(super) fn headers(policy: &Path) -> crate::usage::TokenHeaders {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    token_quota::read_all(&token_quota::token_accounts(policy, &home))
}
