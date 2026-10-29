use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use super::ordered_json::Json;
use super::wiring::{
    EVENT_ORDER, PROVIDERS, bindings, codex_adapters, guards, nix_adapters, render_claude,
    render_codex, text,
};
use super::{Contract, Locations, UNIT, py_list, py_repr};

static GATE_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^#\n]*\bauthoring_guards_off\b[^#\n]*&&[ \t]*exit 0").unwrap()
});
static GATE_SOURCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^#\n]*authoring-killswitch\.sh").unwrap());

const REGENERATE: &str = "north agent-policy-contract --repo <firn> --write";

type HookUnits = BTreeMap<String, (BTreeSet<&'static str>, Option<String>)>;

fn file_name(path: &str) -> &str {
    Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or("")
}

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
            let owner_path = catalog
                .get("registrations")
                .and_then(|registrations| registrations.get(unit))
                .and_then(|registration| registration.get("owner"))
                .map_or("", |owner| text(owner, "path"));
            let identity = if kind == "providerAdapter" {
                distribution.get("adapterId").and_then(Value::as_str).map(str::to_owned)
            } else {
                Some(file_name(owner_path).to_owned())
            };
            let targets: Vec<&str> = distribution
                .get("targets")
                .and_then(Value::as_array)
                .map(|targets| targets.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let slot = units.entry(unit.clone()).or_default();
            slot.0.extend(PROVIDERS.iter().filter(|provider| targets.contains(provider)));
            slot.1 = identity;
        }
    }
    units
}

fn check_hook_source(contract: &mut Contract, unit: &str, source: &Path) {
    if !source.is_file() {
        contract.reject(format!("{unit}: wired hook source is missing: {}", source.display()));
        return;
    }
    let body = std::fs::read_to_string(source).unwrap_or_default();
    if !(GATE_SOURCE.is_match(&body) && GATE_CALL.is_match(&body)) {
        contract.reject(format!(
            "{unit}: wired hook must source lib/authoring-killswitch.sh and exit 0 \
             when authoring_guards_off"
        ));
    }
}

fn check_guard(
    contract: &mut Contract,
    guard: &Value,
    expected: Option<&(BTreeSet<&'static str>, Option<String>)>,
    hooks: &Path,
) {
    let unit = text(guard, "unit");
    let command = text(guard, "command");
    let registered = expected.and_then(|entry| entry.1.as_deref());
    if registered != Some(command) {
        contract.reject(format!(
            "{unit}: command {} is not the hook catalog-config.json registers for it ({})",
            py_repr(command),
            registered.map_or("None".into(), py_repr)
        ));
    }
    for provider in PROVIDERS {
        let custom = text(guard, &format!("{provider}_command"));
        if !custom.is_empty() && file_name(custom.split_whitespace().last().unwrap_or("")) != command {
            contract.reject(format!("{unit}: {provider}_command must run {command}"));
        }
        for binding in bindings(guard, provider) {
            if !EVENT_ORDER.contains(&binding.split(':').next().unwrap_or("")) {
                contract.reject(format!("{unit}: unknown {provider} event in {}", py_repr(binding)));
            }
        }
    }
    let wired: BTreeSet<&str> = PROVIDERS
        .into_iter()
        .filter(|provider| !bindings(guard, provider).is_empty())
        .collect();
    let targets: BTreeSet<&str> = expected.map(|entry| entry.0.iter().copied().collect()).unwrap_or_default();
    if wired != targets {
        contract.reject(format!(
            "{unit}: wired for {} but catalog-config.json targets {}",
            py_list(wired.iter().copied()),
            py_list(targets.iter().copied())
        ));
    }
    for provider in PROVIDERS {
        let absent = !text(guard, &format!("{provider}_absent")).is_empty();
        if wired.contains(provider) == absent {
            contract.reject(format!(
                "{unit}: give {provider}_absent a reason exactly when it has no {provider} binding"
            ));
        }
    }
    check_hook_source(contract, unit, &hooks.join(command));
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
        check_guard(contract, guard, units.get(unit), &locations.hooks);
    }
    for (unit, (targets, _)) in &units {
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
    let adapters = codex_adapters(policy, catalog);
    if nix_adapters(&read(&locations.nix, "Codex Nix module")?) != adapters {
        contract.reject(format!(
            "modules/codex/default.nix providerAdapter list must be exactly the Codex-bound \
             guards plus providerSupport: {}",
            py_list(adapters.iter().map(String::as_str))
        ));
    }
    Ok(())
}
