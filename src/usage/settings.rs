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
    let catalog = catalog();
    let plan = catalog.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|account| account.get("plans")).and_then(|plans| plans.get(&key));
    let named = plan.and_then(|plan| Some((plan.get("name")?.as_str()?.to_owned(), number_text(plan.get("monthly_price")?))));
    match named {
        Some((name, price)) => format!(" | plan {name} ${price}/month"),
        None => format!(" | plan {key}"),
    }
}

pub(super) fn services(operator: &Path) -> Option<Vec<toml_edit::Table>> {
    let document: DocumentMut = fs::read_to_string(operator.join("costs.toml")).ok()?.parse().ok()?;
    Some(document.get("service").and_then(toml_edit::Item::as_array_of_tables).map(|tables| tables.iter().cloned().collect()).unwrap_or_default())
}

pub(super) fn catalog_json() -> serde_json::Value {
    let catalog: DocumentMut = CATALOG.parse().expect("harness/model-catalog.toml parses");
    crate::router::policy::json_value(catalog.as_item())
}

pub(super) fn default_value(name: &str) -> serde_json::Value {
    let defaults: DocumentMut = DEFAULTS.parse().expect("harness/defaults.toml parses");
    crate::router::policy::json_value(&defaults[name]["value"])
}

pub(super) fn catalog() -> DocumentMut {
    CATALOG.parse().expect("harness/model-catalog.toml parses")
}

pub(super) fn machines() -> Vec<(String, Vec<String>)> {
    let catalog = catalog();
    let accounts = catalog.get("accounts").and_then(toml_edit::Item::as_table_like);
    accounts
        .into_iter()
        .flat_map(|accounts| accounts.iter())
        .filter_map(|(provider, account)| {
            let hosts = account.get("machines")?.as_array()?;
            Some((provider.to_owned(), hosts.iter().filter_map(|host| host.as_str().map(str::to_owned)).collect()))
        })
        .collect()
}

pub(super) fn number_text(value: &toml_edit::Item) -> String {
    value.as_integer().map_or_else(|| value.as_float().map_or_else(|| value.as_str().unwrap_or("?").to_owned(), |float| format!("{float:?}")), |integer| integer.to_string())
}
