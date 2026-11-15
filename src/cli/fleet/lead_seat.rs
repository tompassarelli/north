//! A lead launched onto a host that keeps its own fleet but takes no placed session, such as the ledger host nexus: the launch runs in that host's fleet over its deploy destination, so the lead's record, its spool and the tick that places its workers on the other hosts all live there (north#1543).

use std::process::Command;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::SESSION_ENV;
use crate::pacing::placement::{self, config, gate, launch::trusted};

/// The launch `words` run on the lead seat their `--cwd HOST:PATH` names, after this host admits its caller, None when HOST is no such seat; the far fleet admits it as the operator, so only the operator's launch goes.
pub(super) fn forwarded(fleet: &Fleet, words: &[&str], queued: Option<&str>) -> Option<NorthResult<()>> {
    let at = words.iter().position(|word| *word == "--cwd")?;
    let (host, path) = words.get(at + 1)?.split_once(':').filter(|(host, path)| !host.contains('/') && path.starts_with('/'))?;
    if placement::machine(host).is_some() || !trusted(host, fleet.host()) {
        return None;
    }
    let cfg = config().ok()?;
    let destination = gate::ssh_destination(cfg.get("hosts")?.get(host)?.get("deploy_ssh")?.as_str()?)?.to_owned();
    let caller = std::env::var(SESSION_ENV).ok();
    if let Err(error) = fleet.admit_launch_from(caller.as_deref()) {
        return Some(Err(error));
    }
    let operator = super::placement::operator_launcher(fleet, caller.as_deref(), queued);
    let remote = match seat_words(host, words, at, path, operator, caller.as_deref()) {
        Ok(remote) => remote,
        Err(why) => return Some(Err(NorthError::Refused(why))),
    };
    let command = ["north", "fleet", "launch"].map(str::to_owned).into_iter().chain(remote.iter().map(|word| quoted(word))).collect::<Vec<_>>().join(" ");
    Some(match Command::new("ssh").args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &destination, &command]).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(NorthError::State(format!("north fleet launch on {host} over ssh {destination} exited {status}"))),
        Err(error) => Err(NorthError::State(format!("north fleet launch on {host}: ssh {destination} did not start: {error}"))),
    })
}

/// The words `north fleet launch` runs with on `host`: `--cwd` names `path` there and the launch keeps its launcher as parent; only the operator's lead launch is seated.
fn seat_words(host: &str, words: &[&str], at: usize, path: &str, operator: bool, caller: Option<&str>) -> Result<Vec<String>, String> {
    if !operator {
        return Err(format!("--cwd {host}:{path}: only the operator seats a lead on {host}, since {host}'s fleet admits the launch as the operator's own"));
    }
    if !words.windows(2).any(|pair| pair == ["--role", "lead"]) {
        return Err(format!("--cwd {host}:{path}: {host} takes no placed session and seats only leads, which launch there with --role lead"));
    }
    let parent = caller.filter(|caller| !caller.is_empty() && !words.contains(&"--parent")).map(|caller| ["--parent".to_owned(), caller.to_owned()]);
    let named = words.iter().enumerate().map(|(index, word)| if index == at + 1 { path.to_owned() } else { (*word).to_owned() });
    Ok(parent.into_iter().flatten().chain(named).collect())
}

fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
#[path = "lead_seat_tests.rs"]
mod tests;
