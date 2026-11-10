use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use north_coverage::map::state_home;
use serde_json::json;
use toml_edit::{DocumentMut, Item};

use crate::cone::{Cone, Proof};
use crate::facts::TestId;

pub const GATE: &str = "test-selection-proof";

pub fn shadowed(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(root.join("harness/gates.toml")) else { return false };
    let Ok(document) = text.parse::<DocumentMut>() else { return false };
    document.get("gate").and_then(|gates| gates.get(GATE)).and_then(|gate| gate.get("mode")).and_then(Item::as_str) == Some("shadow")
}

fn names<'t>(tests: impl Iterator<Item = &'t TestId>) -> Vec<String> {
    tests.map(ToString::to_string).collect()
}

pub fn record(base: &str, head: &str, cone: &Cone, proof: &Proof) -> Result<(), String> {
    let today: BTreeSet<&TestId> = cone.tests.iter().chain(&cone.always).collect();
    let extra = names(proof.tests.iter().filter(|test| !today.contains(test)));
    let dropped = names(cone.tests.iter().filter(|test| !proof.tests.contains(*test)));
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = json!({ "at": at, "base": base, "head": head, "selected": today.len(), "proof": proof.tests.len(), "extra": extra, "dropped": dropped, "untraced": cone.untraced, "proof_untraced": proof.untraced });
    let directory = state_home().join("north/gates");
    std::fs::create_dir_all(&directory).map_err(|why| format!("cannot create {}: {why}", directory.display()))?;
    let path = directory.join(format!("{GATE}.jsonl"));
    let mut file = OpenOptions::new().create(true).append(true).open(&path).map_err(|why| format!("cannot open {}: {why}", path.display()))?;
    writeln!(file, "{line}").map_err(|why| format!("cannot append to {}: {why}", path.display()))
}
