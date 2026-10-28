use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use toml_edit::DocumentMut;

const CATALOG: &str = include_str!("../../harness/model-catalog.toml");
const DEFAULTS: &str = include_str!("../../harness/defaults.toml");

pub(super) fn known_providers() -> BTreeSet<String> {
    let catalog: DocumentMut = CATALOG.parse().expect("harness/model-catalog.toml parses");
    keys(catalog.get("accounts"))
}

pub(super) fn hosted_claude(policy: &Path) -> BTreeSet<String> {
    let Some(document) = fs::read_to_string(policy).ok().and_then(|text| text.parse::<DocumentMut>().ok()) else {
        return BTreeSet::new();
    };
    keys(document.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("profiles")))
}

pub(super) fn stale_minutes() -> f64 {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    let value = &defaults["usage_stale_min"]["value"];
    value.as_float().or_else(|| value.as_integer().map(|minutes| minutes as f64)).expect("harness/defaults.toml has usage_stale_min")
}

fn keys(table: Option<&toml_edit::Item>) -> BTreeSet<String> {
    table.and_then(toml_edit::Item::as_table_like).map(|table| table.iter().map(|(name, _)| name.to_owned()).collect()).unwrap_or_default()
}
