use std::path::Path;
use std::sync::LazyLock;

use toml_edit::DocumentMut;

use super::super::git;


static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

pub(super) struct Claim {
    pub test: String,
    pub category: String,
}

pub(super) fn categories(directory: &Path, base: &str) -> Vec<String> {
    git::text(directory, &["config", "--blob", &format!("{base}:.safe-push"), "--get-all", "test-admission.contract"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split(" :: ").next().map(str::trim).filter(|category| !category.is_empty()))
        .map(str::to_ascii_lowercase)
        .collect()
}

pub(super) fn unlisted(claims: &[Claim], listed: &[String], patch: &[u8]) -> Vec<String> {
    let added: Vec<&[u8]> = patch.split(|byte| *byte == b'\n').filter(|line| line.first() == Some(&b'+')).collect();
    let named = |test: &str| added.iter().any(|line| line.windows(test.len()).any(|window| window == test.as_bytes()));
    claims
        .iter()
        .filter_map(|claim| {
            if !listed.contains(&claim.category.to_ascii_lowercase()) {
                Some(format!("{} names contract category {}, which is not in this repository's list ({})", claim.test, claim.category, listed_text(listed)))
            } else if !named(&claim.test) {
                Some(format!("{} is claimed as a {} contract test, but no added test line names it", claim.test, claim.category))
            } else {
                None
            }
        })
        .collect()
}

fn listed_text(listed: &[String]) -> String {
    if listed.is_empty() { "none: .safe-push has no test-admission.contract".to_owned() } else { listed.join(", ") }
}

pub(super) fn kill_seconds() -> u64 {
    DEFAULTS["mutant_kill_seconds"]["value"].as_integer().and_then(|seconds| u64::try_from(seconds).ok()).expect("harness/defaults.toml has mutant_kill_seconds")
}
