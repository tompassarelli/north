use std::path::Path;

use super::candidate::git;

pub fn landing(repo: &Path, remote_ref: &str, key: &str) -> String {
    let committed = git(repo, &["config", "--blob", &format!("{remote_ref}:.safe-push"), "--get", key]).unwrap_or_default();
    if committed.is_empty() { git(repo, &["config", "--get", key]).unwrap_or_default() } else { committed }
}

/// The hosts whose outbox the lander pulls and whose lanes safe-push therefore queues: the fleet's ssh hosts in orchestration.toml, unless a test fixture root is declared and the file lies outside it, then any `landing.remote-hosts` a repository or this machine adds.
pub fn outbox_hosts(repo: &Path, remote_ref: &str) -> Vec<String> {
    let fleet = north_base::platform::land_queue::confined(&north_base::platform::orchestration::path()).ok().and_then(|()| north_base::platform::orchestration::read().ok());
    let mut hosts = fleet.map(|fleet| north_base::platform::orchestration::ssh_hosts(&fleet)).unwrap_or_default();
    for host in landing(repo, remote_ref, "landing.remote-hosts").split_whitespace() {
        if !hosts.iter().any(|known| known == host) {
            hosts.push(host.to_owned());
        }
    }
    hosts
}
