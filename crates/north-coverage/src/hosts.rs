//! Telling every host's graph the map and read sets a refresh recorded: the refresh's own host takes them in
//! [`crate::refresh`], and each other build host and the lander is sent the same record output over ssh and takes it
//! in with `north test coverage take`. A host that cannot be reached is named in the refresh's log and
//! keeps its older map; the lander's round line reports the difference (north#1039).

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_base::platform::{host, orchestration, process};

use crate::run_selected::git;

const INGEST: Duration = Duration::from_secs(600);

/// The ssh script that installs a record's output as `repository`'s and takes it in on a host.
/// The state directory is the same relative path a host's own `north test coverage` writes.
pub fn script(repository: &str) -> String {
    format!(
        "d=\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/coverage\"; \
         mkdir -p \"$d\" && cat > \"$d/{repository}.recorded.part\" && mv -f \"$d/{repository}.recorded.part\" \"$d/{repository}.recorded\" && \
         n=\"${{AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}}\" && \"$n\" test coverage take --repo {repository}"
    )
}

/// Send every build host and the lander, other than this host, the record output at `recorded`. Returns one warning
/// per host that was not told, with the local graph already told by the caller.
pub fn tell(root: &Path, recorded: &Path, repository: &str) -> Vec<String> {
    if !plain(repository) {
        return vec![format!("not telling the build hosts: {repository:?} is not a plain repository name")];
    }
    let bytes = match std::fs::read(recorded) {
        Ok(bytes) => bytes,
        Err(error) => return vec![format!("not telling the build hosts: cannot read {}: {error}", recorded.display())],
    };
    let here = host::name();
    let mut warnings = Vec::new();
    let mut planned = planned_hosts(root);
    if planned.is_empty() {
        warnings.push("no build host is configured for the map; only this host's graph was told".to_owned());
    }
    planned.retain(|(name, _)| *name != here);
    for (name, destination) in planned {
        if let Err(why) = ingest_on(&destination, &bytes, repository) {
            warnings.push(format!("{name}'s graph was not told: {why}"));
        }
    }
    warnings
}

/// The build hosts the fleet's orchestration names, plus the lander a repository's `.safe-push`
/// names, so a refresh run from a build host still reaches the lander's graph. A checkout without
/// the private orchestration falls back to the hosts its `.safe-push` lists.
fn planned_hosts(root: &Path) -> Vec<(String, String)> {
    let mut hosts = match orchestration::read() {
        Ok(cfg) => orchestration::build_hosts(&cfg),
        Err(_) => setting(root, "landing.remote-hosts").unwrap_or_default().split_whitespace().map(|name| (name.to_owned(), name.to_owned())).collect(),
    };
    if let Some(lander) = setting(root, "landing.lander-host") {
        hosts.push((lander.clone(), lander));
    }
    let mut seen = BTreeSet::new();
    hosts.retain(|(name, _)| seen.insert(name.clone()));
    hosts
}

fn setting(root: &Path, key: &str) -> Option<String> {
    let value = git(root, &["config", "--file", ".safe-push", "--get", key]).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(crate) fn plain(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
}

fn ingest_on(destination: &str, map: &[u8], repository: &str) -> Result<(), String> {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4", "-T", "--", destination, &script(repository)]);
    match process::try_run_command_traced_within(command, Some(map), INGEST) {
        Ok(Ok(output)) if output.status.success() => Ok(()),
        Ok(Ok(output)) => Err(format!(
            "ssh {destination} exited {}: {}",
            output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Ok(Err(stalled)) => Err(format!("ssh {destination} gave no answer within {} s and was killed: {stalled}", INGEST.as_secs())),
        Err(error) => Err(format!("ssh {destination}: {error}")),
    }
}
