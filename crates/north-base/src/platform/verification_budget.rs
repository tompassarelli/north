use std::path::Path;

const LEDGER: &str = "safe-push/post-land.tsv";
const WINDOW: usize = 10;
const TIMED: usize = 3;

pub fn budget_seconds() -> u64 {
    super::harness::document(super::harness::DEFAULTS)["verification_budget_seconds"]["value"].as_integer().and_then(|seconds| u64::try_from(seconds).ok()).expect("harness/defaults.toml has verification_budget_seconds")
}

pub fn measured_seconds(state_home: &Path, repository: &str) -> Option<u64> {
    let text = std::fs::read_to_string(state_home.join(LEDGER)).ok()?;
    let mut walls: Vec<u64> = text
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            (fields.get(1) == Some(&repository) && fields.get(3) == Some(&"green") && fields.get(7).is_none_or(|covered_by| covered_by.is_empty())).then(|| fields.get(8)?.trim().parse().ok())?
        })
        .collect();
    walls.drain(..walls.len().saturating_sub(WINDOW));
    walls.sort_unstable();
    (walls.len() >= TIMED).then(|| walls[walls.len() / 2])
}

pub fn is_over(state_home: &Path, repository: &str) -> bool {
    measured_seconds(state_home, repository).is_some_and(|measured| measured > budget_seconds())
}
