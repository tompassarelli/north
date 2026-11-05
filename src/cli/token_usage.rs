use std::path::PathBuf;

use serde_json::Value;

pub(super) fn read(policy: &std::path::Path, at: f64) -> Vec<Value> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let accounts = crate::providers::claude::token_quota::token_accounts(policy, &home);
    std::thread::scope(|scope| {
        let jobs: Vec<_> = accounts.iter().map(|account| scope.spawn(move || (account, crate::providers::claude::token_quota::usage_rows(account, at)))).collect();
        jobs.into_iter()
            .filter_map(|job| job.join().ok())
            .flat_map(|(account, read)| {
                read.unwrap_or_else(|error| {
                    eprintln!("pace-control: claude:{} usage by token failed: {error}", account.label);
                    Vec::new()
                })
            })
            .collect()
    })
}

pub(super) fn headers(policy: &std::path::Path) -> crate::usage::TokenHeaders {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let accounts = crate::providers::claude::token_quota::token_accounts(policy, &home);
    std::thread::scope(|scope| {
        let reads: Vec<_> = accounts
            .iter()
            .map(|account| {
                scope.spawn(move || (account.label.clone(), account.token_file.as_deref().ok_or_else(|| "account has no token file".to_owned()).and_then(crate::providers::claude::token_quota::fetch_headers)))
            })
            .collect();
        reads.into_iter().filter_map(|read| read.join().ok()).collect()
    })
}
