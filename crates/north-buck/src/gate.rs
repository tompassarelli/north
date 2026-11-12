use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use toml_edit::{DocumentMut, Item};

const NAME: &str = "buck-targets-drift";
const REGISTRY: &str = "harness/gates.toml";

fn shadowed(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(root.join(REGISTRY)) else {
        return false;
    };
    let Ok(document) = text.parse::<DocumentMut>() else {
        return false;
    };
    document.get("gate").and_then(|gates| gates.get(NAME)).and_then(|gate| gate.get("mode")).and_then(Item::as_str) == Some("shadow")
}

fn record(state: &Path, findings: &[String]) {
    let directory = state.join("north/gates");
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": NAME, "at": at, "would_refuse": !findings.is_empty(), "findings": findings});
    let _ = std::fs::create_dir_all(&directory);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(directory.join(format!("{NAME}.jsonl"))) {
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

/// The exit status the findings earn: 1 when there are any, unless `harness/gates.toml` under `root` keeps the gate in shadow, where each decision is logged under `state` and the findings print as would-be refusals with status 0.
pub fn decide(root: &Path, state: &Path, findings: &[String]) -> u8 {
    let shadow = shadowed(root);
    if shadow {
        record(state, findings);
    }
    let verb = if shadow { "would refuse (shadow)" } else { "refused" };
    for finding in findings {
        eprintln!("{NAME}: {verb}: {finding}");
    }
    if !findings.is_empty() {
        eprintln!("{NAME}: regenerate third-party/BUCK with reindeer as docs/rationale/compile-reuse.md says");
    }
    u8::from(!shadow && !findings.is_empty())
}
