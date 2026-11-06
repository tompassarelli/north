use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::providers::claude::token_quota;

pub(super) fn read(policy: &Path, at: f64) -> Vec<Value> {
    headers(policy)
        .into_iter()
        .flat_map(|(label, headers)| {
            headers.and_then(|headers| token_quota::rows_from(&label, &headers, at)).unwrap_or_else(|error| {
                eprintln!("pace-control: claude:{label} usage by token failed: {error}");
                Vec::new()
            })
        })
        .collect()
}

pub(super) fn headers(policy: &Path) -> crate::usage::TokenHeaders {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    token_quota::read_all(&token_quota::token_accounts(policy, &home))
}
