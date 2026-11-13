use serde_json::{Value, json};

use super::relay::Relay;
use super::route::Route;

pub const KEY_ENV: &str = "NORTH_OPENCODE_KEY";

/// The configuration an opencode child starts with: one provider enabled and one model chosen, so the agent has no other model to switch to; on Go its endpoint is the session relay and its key the relay token, never the paid key.
pub fn content(route: Route, relay: Option<&Relay>) -> Value {
    let mut config = json!({
        "$schema": "https://opencode.ai/config.json",
        "enabled_providers": [route.provider()],
        "model": route.model(),
        "small_model": route.model(),
        "autoupdate": false,
        "share": "disabled",
    });
    if let Some(relay) = relay {
        config["provider"] = json!({route.provider(): {"options": {"baseURL": relay.api(), "apiKey": relay.token()}}});
    }
    config
}
