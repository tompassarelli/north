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

pub(super) fn plan_text(policy: &Path, provider: &str, account: Option<&str>, observed: Option<&str>) -> String {
    let declared = || {
        let document: DocumentMut = fs::read_to_string(policy).ok()?.parse().ok()?;
        let plans = document.get("accounts")?.get(provider)?.get("plans")?.as_table_like()?;
        plans.get(account?)?.as_str().map(str::to_owned)
    };
    let Some(key) = observed.filter(|key| !key.is_empty()).map(str::to_owned).or_else(declared) else {
        return String::new();
    };
    let catalog: DocumentMut = CATALOG.parse().expect("harness/model-catalog.toml parses");
    let plan = catalog.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|account| account.get("plans")).and_then(|plans| plans.get(&key));
    let named = plan.and_then(|plan| Some((plan.get("name")?.as_str()?.to_owned(), plan.get("monthly_price")?.clone())));
    match named {
        Some((name, price)) => {
            let price = price.as_integer().map_or_else(|| price.as_float().map_or_else(|| price.as_str().unwrap_or("?").to_owned(), |float| format!("{float:?}")), |integer| integer.to_string());
            format!(" | plan {name} ${price}/month")
        }
        None => format!(" | plan {key}"),
    }
}

pub(super) fn services(operator: &Path) -> Option<Vec<toml_edit::Table>> {
    let document: DocumentMut = fs::read_to_string(operator.join("costs.toml")).ok()?.parse().ok()?;
    Some(document.get("service").and_then(toml_edit::Item::as_array_of_tables).map(|tables| tables.iter().cloned().collect()).unwrap_or_default())
}
