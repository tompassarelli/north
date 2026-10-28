//! The ledger's outside effects: gitleaks before text is stored or posted, gh, the fleet's live
//! sessions and the archive tags naming an issue.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::process::{Command, Stdio};

use serde_json::Value;

use super::claims::Issue;
use super::claims::issues::{GitHub, Issues};

pub(crate) struct Live {
    north: String,
    relayed_owners: Vec<String>,
    archives: bool,
}

impl Live {
    /// The effects as this caller's environment shapes them: which north to ask for live sessions,
    /// the owners a remote caller relayed, and whether the caller prints archives itself.
    pub(crate) fn new(env: &BTreeMap<String, String>) -> Self {
        let var = |name: &str| env.get(name).map(String::as_str).filter(|value| !value.is_empty());
        let relayed_owners = var("THREADS_ALIVE_OWNERS")
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value.as_array().map(|owners| owners.iter().filter_map(Value::as_str).map(str::to_owned).collect()))
            .unwrap_or_default();
        Self { north: var("NORTH_BIN").unwrap_or("north").to_owned(), relayed_owners, archives: var("THREADS_ARCHIVES_LOCAL").is_none() }
    }
}

impl Issues for Live {
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue> {
        GitHub.read(items)
    }
}

impl super::verb::Outside for Live {
    fn scan(&self, text: &str) -> Result<(), String> {
        gitleaks_clean(text)
    }

    fn gh(&self, args: &[String]) -> Result<String, String> {
        crate::platform::github::run_patiently(args, "NorthThreads")
    }

    fn alive_owners(&self) -> BTreeSet<String> {
        let mut owners = fleet_owners(&self.north);
        owners.extend(self.relayed_owners.iter().cloned());
        owners
    }

    fn archives(&self, repo: &str, number: u64) -> String {
        if self.archives { crate::platform::archive_tags::report(repo, number, None) } else { String::new() }
    }
}

/// The names, conversations and provider:conversation ids of this host's sessions that have not
/// exited, as `north fleet list --json` shows them.
pub(crate) fn fleet_owners(north: &str) -> BTreeSet<String> {
    let mut owners = BTreeSet::new();
    let Ok(output) = Command::new(north).args(["fleet", "list", "--json"]).output() else { return owners };
    if !output.status.success() {
        return owners;
    }
    let Some(sessions) = serde_json::from_slice::<Value>(&output.stdout).ok().and_then(|value| value.as_array().cloned()) else { return owners };
    for session in sessions.iter().filter(|session| session.get("status").and_then(Value::as_str) != Some("exited")) {
        let text = |key: &str| session.get(key).and_then(Value::as_str).filter(|value| !value.is_empty());
        owners.extend(text("name").map(str::to_owned));
        owners.extend(text("conversation").map(str::to_owned));
        if let (Some(provider), Some(conversation)) = (text("provider"), text("conversation")) {
            owners.insert(format!("{provider}:{conversation}"));
        }
    }
    owners
}

pub(crate) fn gitleaks_clean(text: &str) -> Result<(), String> {
    let mut command = Command::new("gitleaks");
    command.args(["stdin", "--no-banner", "--redact", "-l", "error"]).current_dir("/").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err("gitleaks is not on PATH; items are scanned for secrets before they are stored".into());
        }
        Err(error) => return Err(error.to_string()),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let bytes = text.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let output = child.wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored".into());
    }
    Ok(())
}
