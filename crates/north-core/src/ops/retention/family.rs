use std::collections::BTreeSet;
use std::time::Duration;

use toml_edit::DocumentMut;

const DEFAULTS: &str = include_str!("../../../../../harness/defaults.toml");

pub(super) struct Family {
    pub(super) name: String,
    pub(super) base: String,
    pub(super) dir: String,
    pub(super) max_age: Duration,
    pub(super) pins: BTreeSet<String>,
    pub(super) files_are_entries: bool,
}

const BASE: &str = "\"$HOME/.local/state/smashcraft\"";

fn setting(name: &str) -> f64 {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    let value = &defaults[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub(super) fn hours(name: &str) -> Duration {
    Duration::from_secs((setting(name) * 3600.0) as u64)
}

pub(super) fn minutes(name: &str) -> Duration {
    Duration::from_secs((setting(name) * 60.0) as u64)
}

pub(super) fn families() -> Vec<Family> {
    vec![
        Family::new("evidence", BASE, "evidence", hours("evidence_retention_hours")),
        Family::new("capture-batches", BASE, "capture-queue/batches", hours("capture_batch_retention_hours")),
    ]
}

impl Family {
    pub(super) fn new(name: &str, base: &str, dir: &str, max_age: Duration) -> Self {
        Self { name: name.into(), base: base.into(), dir: dir.into(), max_age, pins: BTreeSet::new(), files_are_entries: false }
    }

    pub(super) fn enter(&self) -> String {
        format!("cd {} && cd {}", self.base, self.dir)
    }
}

pub(super) fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with(['.', '-']) && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
