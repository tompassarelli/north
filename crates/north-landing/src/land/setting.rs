use std::path::Path;

use super::candidate::git;

pub fn landing(repo: &Path, remote_ref: &str, key: &str) -> String {
    let committed = git(repo, &["config", "--blob", &format!("{remote_ref}:.safe-push"), "--get", key]).unwrap_or_default();
    if committed.is_empty() { git(repo, &["config", "--get", key]).unwrap_or_default() } else { committed }
}

/// The hosts whose outbox the lander pulls and whose lanes safe-push therefore queues: the fleet's ssh hosts in orchestration.toml, unless a test fixture root is declared and the file lies outside it, then any `landing.remote-hosts` a repository or this machine adds.
pub fn outbox_hosts(repo: &Path, remote_ref: &str) -> Vec<String> {
    let mut hosts = super::after::route::fleet_hosts();
    for host in landing(repo, remote_ref, "landing.remote-hosts").split_whitespace() {
        if !hosts.iter().any(|known| known == host) {
            hosts.push(host.to_owned());
        }
    }
    hosts
}

/// Why this host's shared-config snapshot, where the fleet's hosts are read, may not be read: past its bound or never fetched.
pub fn stale_fleet() -> Option<String> {
    use north_base::platform::shared_config;
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    if !shared_config::current(&home).is_dir() {
        return None;
    }
    shared_config::fresh(&home, shared_config::now()).err()
}
