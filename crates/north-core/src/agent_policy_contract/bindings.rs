use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::ordered_json::Json;
use super::wiring::{
    EVENT_ORDER, PROVIDERS, bindings, codex_adapters, guards, nix_adapters, render_claude,
    render_codex, text,
};
use super::{Contract, Locations, UNIT, py_list, py_repr};

const REGENERATE: &str = "north agent-policy-contract --repo <firn> --write";

type HookUnits = BTreeMap<String, BTreeSet<&'static str>>;

fn hook_units(catalog: &Value) -> HookUnits {
    let mut units = HookUnits::new();
    let activation = catalog.get("activation").and_then(Value::as_object);
    for (unit, entry) in activation.into_iter().flatten() {
        let distributions = entry.get("distributions").and_then(Value::as_array);
        for distribution in distributions.into_iter().flatten() {
            let kind = text(distribution, "type");
            if kind != "hook" && kind != "providerAdapter" {
                continue;
            }
            let targets: Vec<&str> = distribution
                .get("targets")
                .and_then(Value::as_array)
                .map(|targets| targets.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            units.entry(unit.clone()).or_default().extend(PROVIDERS.iter().filter(|provider| targets.contains(provider)));
        }
    }
    units
}

fn check_events(contract: &mut Contract, guard: &Value, unit: &str) {
    for provider in PROVIDERS {
        for binding in bindings(guard, provider) {
            if !EVENT_ORDER.contains(&binding.split(':').next().unwrap_or("")) {
                contract.reject(format!("{unit}: unknown {provider} event in {}", py_repr(binding)));
            }
        }
    }
}

fn check_absent_reasons(contract: &mut Contract, guard: &Value, unit: &str) {
    for provider in PROVIDERS {
        let absent = !text(guard, &format!("{provider}_absent")).is_empty();
        if bindings(guard, provider).is_empty() != absent {
            contract.reject(format!(
                "{unit}: give {provider}_absent a reason exactly when it has no {provider} binding"
            ));
        }
    }
}

fn check_north_hook(contract: &mut Contract, guard: &Value, registration: Option<&Value>) {
    let unit = text(guard, "unit");
    if text(guard, "north_hook").is_empty() {
        contract.reject(format!("{unit}: name the north hook verb it runs in north_hook"));
    }
    let custom = !text(guard, "command").is_empty()
        || PROVIDERS.iter().any(|provider| !text(guard, &format!("{provider}_command")).is_empty());
    if custom {
        contract.reject(format!("{unit}: north_hook runs north hook directly, so it takes no command or *_command"));
    }
    let owner = registration.and_then(|entry| entry.get("owner")).map_or("", |owner| text(owner, "path"));
    if !owner.ends_with(".rs") {
        contract.reject(format!("{unit}: north_hook needs a catalog registration owned by a Rust module"));
    }
    check_events(contract, guard, unit);
    check_absent_reasons(contract, guard, unit);
}

fn read(path: &Path, what: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{what} is unreadable: {}: {error}", path.display()))
}

pub(super) fn claude_projection(path: &Path) -> Result<(Vec<(String, Json)>, String), String> {
    let text = read(path, "Claude hook projection")?;
    match serde_json::from_str::<Json>(&text) {
        Ok(Json::Object(entries)) => {
            Ok((entries.into_iter().filter(|(key, _)| key != "hooks").collect(), text))
        }
        Ok(_) => Err("Claude hook projection is not a JSON object".into()),
        Err(error) => Err(format!("Claude hook projection is unreadable: {error}")),
    }
}

pub(super) fn check(
    contract: &mut Contract,
    policy: &Value,
    catalog: &Value,
    locations: &Locations,
) -> Result<(), String> {
    let units = hook_units(catalog);
    let mut seen = BTreeSet::new();
    for guard in guards(policy) {
        let unit = text(guard, "unit");
        if !UNIT.is_match(unit) || !seen.insert(unit) {
            contract.reject(format!("invalid or duplicate provider guard unit: {}", py_repr(unit)));
            continue;
        }
        let registration = catalog.get("registrations").and_then(|registrations| registrations.get(unit));
        check_north_hook(contract, guard, registration);
        let wired: BTreeSet<&str> = PROVIDERS.into_iter().filter(|provider| !bindings(guard, provider).is_empty()).collect();
        let targets: BTreeSet<&str> = units.get(unit).map(|targets| targets.iter().copied().collect()).unwrap_or_default();
        if !targets.is_empty() && wired != targets {
            contract.reject(format!(
                "{unit}: wired for {} but catalog-config.json targets {}",
                py_list(wired.iter().copied()),
                py_list(targets.iter().copied())
            ));
        }
    }
    for (unit, targets) in &units {
        if !targets.is_empty() && !seen.contains(unit.as_str()) {
            contract.reject(format!(
                "{unit}: registered hook targets {} but is unwired",
                py_list(targets.iter().copied())
            ));
        }
    }
    match claude_projection(&locations.claude) {
        Ok((settings, current)) => {
            if current != render_claude(policy, settings) {
                contract.reject(format!(
                    "{} differs from guard[]; run {REGENERATE}",
                    locations.claude.display()
                ));
            }
        }
        Err(error) => contract.reject(error),
    }
    if read(&locations.codex, "Codex requirements")? != render_codex(policy) {
        contract.reject(format!("{} differs from guard[]; run {REGENERATE}", locations.codex.display()));
    }
    let adapters = codex_adapters(catalog);
    if nix_adapters(&read(&locations.nix, "Codex Nix module")?) != adapters {
        contract.reject(format!(
            "modules/codex/default.nix providerAdapter list must be exactly the Codex-bound \
             guards plus providerSupport: {}",
            py_list(adapters.iter().map(String::as_str))
        ));
    }
    Ok(())
}
