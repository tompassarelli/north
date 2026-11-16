//! The fleet table this host caches for `north fleet tree`. The runtime's tick refreshes it, and `north fleet off`
//! stops every tick, so it parks the rows of sessions it stopped itself: a row whose host, and whose machine when it
//! names one, `off` left with no fleet unit running.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

const FILE: &str = "north/fleet/.fleet-sessions.json";
const LIVE: [&str; 3] = ["working", "starting", "idle"];
const PARKED: &str = "parked";

pub fn park(state_home: &Path, stopped_hosts: &BTreeSet<String>, caller: Option<&str>) -> Result<Vec<String>, String> {
    let path = state_home.join(FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut rows: Value = serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let field = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut parked = Vec::new();
    for row in rows.as_array_mut().into_iter().flatten() {
        let (name, host, machine, status) = (field(row, "name"), field(row, "host"), field(row, "machine"), field(row, "status"));
        let stopped = stopped_hosts.contains(&host) && (machine.is_empty() || stopped_hosts.contains(&machine));
        if stopped && LIVE.contains(&status.as_str()) && caller != Some(name.as_str()) {
            row["status"] = Value::from(PARKED);
            parked.push(name);
        }
    }
    if parked.is_empty() {
        return Ok(parked);
    }
    let staged = path.with_extension("json.new");
    fs::write(&staged, rows.to_string()).and_then(|()| fs::rename(&staged, &path)).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(parked)
}
