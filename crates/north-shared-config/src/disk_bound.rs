//! The free GiB a host's disk is held to, read from the operator policy (orchestration.toml).
use toml_edit::{DocumentMut, Item};

/// The `[hosts.NAME] disk_free_gib` the policy sets for `host`. A host the policy names as `[placement] dispatch_host` and that
/// sets none gets `dispatch_default`; any other host sets none gets nothing.
pub fn bound(policy: &DocumentMut, host: &str, dispatch_default: f64) -> Option<f64> {
    match policy.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|table| table.get("disk_free_gib")) {
        Some(value) => number(value),
        None => (dispatch_host(policy) == Some(host)).then_some(dispatch_default),
    }
}

fn number(value: &Item) -> Option<f64> {
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64))
}

fn dispatch_host(policy: &DocumentMut) -> Option<&str> {
    policy.get("placement")?.get("dispatch_host")?.as_str()
}
