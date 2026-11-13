use std::path::Path;

use serde_json::Value;

use crate::fleet::model::EntryKind;

use super::super::opencode::harness::{Harness, Tally};
use super::super::opencode::route::Route;
use super::super::opencode::turns::Host;
use super::events;

pub const PROVIDER: &str = "pi";
pub const KEY_ENV: &str = "OPENCODE_API_KEY";
const PUBLIC_KEY: &str = "public";
const SETTINGS_FILE: &str = "settings.json";

pub struct Pi;

pub static PI: Pi = Pi;

impl Harness for Pi {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn host_verb(&self) -> &'static str {
        "pi-host"
    }

    fn conversation(&self, uuid: &str) -> String {
        uuid.to_owned()
    }

    fn words(&self, session: &str, route: Route, _effort: Option<&str>, text: &str) -> Vec<String> {
        let model = route.model().split_once('/').map_or(route.model(), |(_, model)| model);
        ["--mode", "json", "--offline", "--no-approve", "--no-mcp", "--provider", route.provider(), "--model", model, "--session-id", session, "--", text].map(str::to_owned).into()
    }

    fn key_env(&self) -> &'static str {
        KEY_ENV
    }

    fn environment(&self, host: &Host, _route: Route, key: Option<&str>) -> Vec<(String, String)> {
        let agent = host.scratch.join("agent");
        vec![
            ("PI_CODING_AGENT_DIR".to_owned(), agent.display().to_string()),
            ("PI_OFFLINE".to_owned(), "1".to_owned()),
            ("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned()),
            ("PI_TELEMETRY".to_owned(), "0".to_owned()),
            (KEY_ENV.to_owned(), key.unwrap_or(PUBLIC_KEY).to_owned()),
        ]
    }

    /// Pi starts its shell tool from the `shellPath` of its agent directory's settings.json, else /bin/bash or bash on PATH, never `$SHELL`.
    fn bind_shell(&self, host: &Host, shell: &Path) -> std::io::Result<()> {
        let agent = host.scratch.join("agent");
        std::fs::create_dir_all(&agent)?;
        let file = agent.join(SETTINGS_FILE);
        let mut settings = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).filter(Value::is_object).unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        settings["shellPath"] = Value::String(shell.display().to_string());
        std::fs::write(file, settings.to_string())
    }

    fn read(&self, tally: &mut Tally, line: &str) {
        if let Ok(event) = serde_json::from_str::<Value>(line) {
            events::read(tally, &event);
        }
    }

    fn entries(&self, event: &Value) -> Vec<(EntryKind, String)> {
        events::entries(event)
    }

    fn skills(&self) -> &'static str {
        "agent/skills"
    }
}
