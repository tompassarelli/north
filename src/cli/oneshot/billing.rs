//! What routes `claude` to billing outside the subscription: the environment variables a one-shot ask and a plan
//! session unset, and the Claude config directory settings that would set them again or fetch a key.
use std::path::Path;

/// Environment that routes `claude` to billing outside the subscription.
pub const EXTERNAL: [&str; 6] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY"];

/// The JSON file `file` in `dir`, or none when it is missing or unreadable.
pub fn read_json(dir: &Path, file: &str) -> Option<serde_json::Value> {
    std::fs::read_to_string(dir.join(file)).ok().and_then(|held| serde_json::from_str(&held).ok())
}

/// Why the Claude config directory `dir` would bill by token or reach another endpoint, if it would: an apiKeyHelper, or an env block naming an API key, token or endpoint variable, in its settings.
pub fn settings_refusal(dir: &Path) -> Option<String> {
    ["settings.json", "settings.local.json"].into_iter().filter_map(|file| read_json(dir, file)).find_map(|settings| {
        let named = EXTERNAL.into_iter().find(|name| settings.get("env").and_then(|env| env.get(name)).is_some());
        match (settings.get("apiKeyHelper"), named) {
            (Some(_), _) => Some(format!("an apiKeyHelper is configured in {}, so it would bill by token", dir.display())),
            (None, Some(name)) => Some(format!("{name} is set in the settings of {}, so it would bill by token", dir.display())),
            (None, None) => None,
        }
    })
}
