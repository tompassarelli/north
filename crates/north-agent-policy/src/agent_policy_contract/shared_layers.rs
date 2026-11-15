use serde_json::Value;

use super::wiring::{bindings, text};
use super::{Contract, py_list, py_repr};

const HOOKLESS_PROVIDERS: [&str; 3] = ["opencode", "pi", "agy"];
const SHARED_LAYERS: [&str; 6] = ["launch", "worker-sandbox", "sandbox-broker", "safe-push", "lander", "github-app"];

pub(super) fn check(contract: &mut Contract, guard: &Value, unit: &str) {
    for provider in HOOKLESS_PROVIDERS {
        let layers = bindings(guard, &format!("{provider}_layer"));
        let absent = !text(guard, &format!("{provider}_absent")).is_empty();
        if layers.is_empty() != absent {
            contract.reject(format!(
                "{unit}: {provider} has no hook, so name the shared layer that enforces it in {provider}_layer or give {provider}_absent a reason, not both"
            ));
        }
        for layer in layers {
            if !SHARED_LAYERS.contains(&layer) {
                contract.reject(format!(
                    "{unit}: unknown {provider}_layer {}; shared layers are {}",
                    py_repr(layer),
                    py_list(SHARED_LAYERS.into_iter())
                ));
            }
        }
    }
}
