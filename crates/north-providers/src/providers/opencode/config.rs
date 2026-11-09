use serde_json::{Value, json};

use super::route::Route;

pub const KEY_ENV: &str = "NORTH_OPENCODE_KEY";

/// The configuration an opencode child starts with: one provider enabled and one model chosen, so the agent has no other model to switch to.
pub fn content(route: Route) -> Value {
    let mut config = json!({
        "$schema": "https://opencode.ai/config.json",
        "enabled_providers": [route.provider()],
        "model": route.model(),
        "small_model": route.model(),
        "autoupdate": false,
        "share": "disabled",
    });
    if route == Route::Go {
        config["provider"] = json!({route.provider(): {"options": {"apiKey": format!("{{env:{KEY_ENV}}}")}}});
    }
    config
}
