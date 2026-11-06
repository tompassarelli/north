//! The rate limit GitHub last reported on each token the gh broker carried, left on disk for the
//! decisions that spend GitHub calls to log (docs/rationale/strategy-kernel.md, Replay).
use std::fs;
use std::path::Path;

use north_strategy_types::github::Quota;

/// Keeps the reading a response carried for one token and rate-limit resource.
pub fn record(directory: &Path, token: &str, resource: &str, remaining: u64, resets_at: u64) {
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let name = format!("{resource}-{}", super::decision_log::sha256_hex(token.as_bytes()).chars().take(16).collect::<String>());
    let staged = directory.join(format!("{name}.{}", std::process::id()));
    if fs::write(&staged, format!("{remaining} {resets_at}\n")).is_ok() {
        let _ = fs::rename(&staged, directory.join(name));
    }
}

/// The lowest reading the running broker left, None when it has seen none in a live window.
pub fn seen(now: f64) -> Option<Quota> {
    lowest(&super::gh_broker_route::Place::from_env().quota(), now)
}

/// The reading with the fewest calls left among the windows still open at `now`.
pub fn lowest(directory: &Path, now: f64) -> Option<Quota> {
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (resource, _) = name.split_once('-')?;
            let text = fs::read_to_string(entry.path()).ok()?;
            let (remaining, resets_at) = text.split_once(' ')?;
            Some(Quota { resource: resource.to_owned(), remaining: remaining.trim().parse().ok()?, resets_at: resets_at.trim().parse().ok()? })
        })
        .filter(|quota| quota.resets_at > now)
        .min_by_key(|quota| quota.remaining)
}
