//! The ledger's outside effects: gitleaks before text is stored or posted, gh, the fleet's live
//! sessions and the archive tags naming an issue.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::Value;

use super::claims::Issue;
use super::claims::issues::{GitHub, Issues};

/// What the ledger reads from this host's fleet.
pub(crate) trait FleetView: Send + Sync {
    /// The owners of the sessions alive here, `None` when the registry could not be read.
    fn live_owners(&self) -> Option<BTreeSet<String>>;
    /// Each issue an open gate here or on a relayed host decides, with the gate's label.
    fn gated_issues(&self) -> BTreeMap<String, String>;
}

/// A fleet that cannot be read: no owner is known and no gate holds an issue.
pub(crate) struct Unread;

impl FleetView for Unread {
    fn live_owners(&self) -> Option<BTreeSet<String>> {
        None
    }

    fn gated_issues(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}

pub(crate) struct Live {
    fleet: Arc<dyn FleetView>,
    relayed_owners: Vec<String>,
    archives: bool,
    operator: PathBuf,
}

impl Live {
    /// The effects as this caller's environment shapes them: where the live sessions come from,
    /// the owners a remote caller relayed, whether the caller prints archives itself, and the
    /// operator directory holding costs.toml.
    pub(crate) fn new(env: &BTreeMap<String, String>, fleet: Arc<dyn FleetView>) -> Self {
        let var = |name: &str| env.get(name).map(String::as_str).filter(|value| !value.is_empty());
        let relayed_owners = var("THREADS_ALIVE_OWNERS")
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| value.as_array().map(|owners| owners.iter().filter_map(Value::as_str).map(str::to_owned).collect()))
            .unwrap_or_default();
        let operator = var("AGENTS_OPERATOR_DIR").map_or_else(|| PathBuf::from(var("HOME").unwrap_or_default()).join("code/south/main/agents"), PathBuf::from);
        Self { fleet, relayed_owners, archives: var("THREADS_ARCHIVES_LOCAL").is_none(), operator }
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

    fn alive_owners(&self) -> Option<BTreeSet<String>> {
        let fleet = self.fleet.live_owners();
        if fleet.is_none() && self.relayed_owners.is_empty() {
            return None;
        }
        let mut owners = fleet.unwrap_or_default();
        owners.extend(self.relayed_owners.iter().cloned());
        Some(owners)
    }

    fn gated_issues(&self) -> BTreeMap<String, String> {
        self.fleet.gated_issues()
    }

    fn archives(&self, repo: &str, number: u64) -> String {
        if self.archives { crate::platform::archive_tags::report(repo, number, None) } else { String::new() }
    }

    fn costs_path(&self) -> PathBuf {
        self.operator.join("costs.toml")
    }

    fn prices(&self) -> Result<BTreeMap<String, f64>, String> {
        let path = self.operator.join("costs.toml");
        let unreadable = |why: String| format!("{}: {why}", path.display());
        let text = std::fs::read_to_string(&path).map_err(|error| unreadable(error.to_string()))?;
        let document: toml_edit::DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| unreadable(error.to_string()))?;
        let Some(services) = document.get("service") else { return Ok(BTreeMap::new()) };
        let services = services.as_array_of_tables().ok_or_else(|| unreadable("service is not an array of tables".into()))?;
        services
            .iter()
            .map(|service| {
                let id = service.get("id").and_then(|id| id.as_str()).ok_or_else(|| unreadable("a service has no id".into()))?;
                let usd = service.get("usd_per_week").and_then(|usd| usd.as_float().or_else(|| usd.as_integer().map(|usd| usd as f64)));
                Ok((id.to_owned(), usd.ok_or_else(|| unreadable(format!("service {id} has no usd_per_week")))?))
            })
            .collect()
    }
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
