use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::request::id;

/// Each Warcraft client whose cgroup lies in no active lease's scope, named by its unit and, where a fleet session's unit holds it, that session.
pub(super) fn unleased_native(clients: &[Value], leases: &[Value], units: &BTreeMap<String, String>) -> Vec<Value> {
    let scopes: Vec<String> = leases.iter().map(|lease| format!("agent-capacity-{}.scope", id(lease).replace('-', ""))).collect();
    clients
        .iter()
        .filter_map(|client| {
            let cgroup = client["cgroup"].as_str().unwrap_or_default();
            let parts: Vec<&str> = cgroup.split('/').filter(|part| !part.is_empty()).collect();
            if parts.iter().any(|part| scopes.iter().any(|scope| scope == part)) {
                return None;
            }
            let owner = units.iter().find(|(unit, _)| parts.contains(&unit.as_str())).map(|(_, session)| session.clone());
            Some(json!({ "pid": client["pid"], "prefix": client["prefix"], "cgroup": cgroup, "unit": parts.last(), "owner": owner }))
        })
        .collect()
}

/// The unit each fleet session on this host runs in, from its `host.scope`, keyed to the session name.
pub(super) fn fleet_units(home: &Path) -> BTreeMap<String, String> {
    fs::read_dir(home.join(".local/state/north/fleet"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((fs::read_to_string(entry.path().join("host.scope")).ok()?.trim().to_owned(), entry.file_name().to_string_lossy().into_owned())))
        .filter(|(unit, _)| !unit.is_empty())
        .collect()
}
