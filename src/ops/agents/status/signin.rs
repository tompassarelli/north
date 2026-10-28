use std::env;
use std::time::Duration;

use serde_json::Value;

use crate::platform::process::run_within;

pub(super) fn check(provider: &str) -> (bool, &'static str) {
    if provider != "claude" {
        return (false, "no sign-in check for this provider");
    }
    let program = env::var("AGENTS_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_owned());
    let signed_in = run_within(&program, &["auth", "status"], Duration::from_secs(30))
        .filter(|output| output.status.success())
        .and_then(|output| {
            let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            serde_json::from_str::<Value>(&text).ok()
        })
        .is_some_and(|status| status["loggedIn"] == Value::Bool(true));
    (signed_in, "claude auth status")
}
