use std::time::Duration;

use toml_edit::DocumentMut;

const DEFAULTS: &str = include_str!("../../../harness/defaults.toml");

pub(super) struct Family {
    pub(super) name: &'static str,
    pub(super) base: String,
    pub(super) dir: &'static str,
    pub(super) max_age: Duration,
}

const BASE: &str = "\"$HOME/.local/state/smashcraft\"";

pub(super) fn families() -> Vec<Family> {
    let hours = |name: &str| {
        let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
        let value = &defaults[name]["value"];
        let hours = value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"));
        Duration::from_secs((hours * 3600.0) as u64)
    };
    vec![
        Family { name: "evidence", base: BASE.into(), dir: "evidence", max_age: hours("evidence_retention_hours") },
        Family { name: "capture-batches", base: BASE.into(), dir: "capture-queue/batches", max_age: hours("capture_batch_retention_hours") },
    ]
}

impl Family {
    pub(super) fn enter(&self) -> String {
        format!("cd {} && cd {}", self.base, self.dir)
    }
}

pub(super) fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with(['.', '-']) && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
