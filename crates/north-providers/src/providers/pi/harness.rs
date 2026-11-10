use serde_json::Value;

use crate::fleet::model::EntryKind;

use super::super::opencode::harness::{Harness, Tally};
use super::super::opencode::route::Route;
use super::super::opencode::turns::Host;
use super::events;

pub const PROVIDER: &str = "pi";
const PUBLIC_KEY: &str = "public";

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

    fn environment(&self, host: &Host, _route: Route, key: Option<&str>) -> Vec<(String, String)> {
        let agent = host.scratch.join("agent");
        vec![
            ("PI_CODING_AGENT_DIR".to_owned(), agent.display().to_string()),
            ("PI_OFFLINE".to_owned(), "1".to_owned()),
            ("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned()),
            ("PI_TELEMETRY".to_owned(), "0".to_owned()),
            ("OPENCODE_API_KEY".to_owned(), key.unwrap_or(PUBLIC_KEY).to_owned()),
        ]
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
