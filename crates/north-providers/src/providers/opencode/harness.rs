use std::process::Command;

use serde_json::Value;

use crate::fleet::model::{Effort, EntryKind};
use crate::fleet::runtime::summarize;
use crate::platform::harness;

use super::config;
use super::route::Route;
use super::turns::Host;

const TOOL_CHARS: usize = 400;

/// What a turn has shown so far, read from the events its agent prints: the tokens it spent, whether it did any work, and the error it ended on.
#[derive(Default)]
pub struct Tally {
    pub tokens: [u64; 4],
    pub progressed: bool,
    pub error: Option<String>,
}

impl Tally {
    pub fn spend(&mut self, input: u64, output: u64, cache_read: u64, cache_write: u64) {
        for (total, count) in self.tokens.iter_mut().zip([input, output, cache_read, cache_write]) {
            *total += count;
        }
    }
}

/// One agent program that runs DeepSeek on a lane: how its turn command is spelled, which environment it needs, and how its JSON events read.
pub trait Harness: Send + Sync {
    /// The provider label its workers carry in the fleet and the ledger.
    fn provider(&self) -> &'static str;

    /// The fleet verb that serves its turns.
    fn host_verb(&self) -> &'static str;

    /// The conversation id of a fresh session, made from a UUID.
    fn conversation(&self, uuid: &str) -> String;

    /// The words after the program that run one turn of `text` in the session on `route` at `effort`.
    fn words(&self, session: &str, route: Route, effort: Option<&str>, text: &str) -> Vec<String>;

    /// The reasoning efforts this program runs at, which a launch draws from; empty for a program that takes none.
    fn effort_arms(&self) -> &'static [Effort] {
        &[]
    }

    /// The variables its child needs on `route`, given the paid key when the route is Go.
    fn environment(&self, host: &Host, route: Route, key: Option<&str>) -> Vec<(String, String)>;

    /// Folds one event line of its output into the turn's tally.
    fn read(&self, tally: &mut Tally, line: &str);

    /// The transcript entries a passed-through event adds.
    fn entries(&self, event: &Value) -> Vec<(EntryKind, String)>;

    /// Where under its scratch directory it discovers skills.
    fn skills(&self) -> &'static str;

    /// The settings file that holds how many of its workers may run at once.
    fn settings(&self) -> &'static str;

    /// The table in that file that holds the cap.
    fn slots_table(&self) -> &'static str;
}

impl dyn Harness {
    /// How many of its workers may run at once, apart from the Claude worker cap.
    pub fn slots(&self) -> usize {
        harness::document(self.settings())[self.slots_table()]["slots"].as_integer().and_then(|slots| usize::try_from(slots).ok()).unwrap_or(1)
    }
}

pub fn tool_text(tool: &str, input: &Value) -> String {
    let detail = ["command", "filePath", "path", "pattern", "description"].iter().find_map(|key| input[*key].as_str()).map_or_else(|| input.to_string(), str::to_owned);
    format!("{tool} {}", summarize(&detail, TOOL_CHARS))
}

pub fn apply(command: &mut Command, variables: Vec<(String, String)>) {
    command.envs(variables);
}

pub struct OpenCode;

pub static OPENCODE: OpenCode = OpenCode;

impl Harness for OpenCode {
    fn provider(&self) -> &'static str {
        "opencode"
    }

    fn host_verb(&self) -> &'static str {
        "opencode-host"
    }

    fn conversation(&self, uuid: &str) -> String {
        format!("ses_{}", uuid.replace('-', "").chars().take(26).collect::<String>())
    }

    fn words(&self, session: &str, route: Route, effort: Option<&str>, text: &str) -> Vec<String> {
        let model = effort.map_or_else(|| route.model().to_owned(), |effort| format!("{}#{effort}", route.model()));
        vec!["run".into(), "--standalone".into(), "--format".into(), "json".into(), "--auto".into(), "-m".into(), model, "-s".into(), session.into(), text.into()]
    }

    fn effort_arms(&self) -> &'static [Effort] {
        &[Effort::High, Effort::Max]
    }

    fn environment(&self, host: &Host, route: Route, key: Option<&str>) -> Vec<(String, String)> {
        let mut config = config::content(route);
        config["shell"] = Value::String(host.scratch.join("shell").display().to_string());
        let mut variables = vec![("OPENCODE_CONFIG_CONTENT".to_owned(), config.to_string())];
        variables.extend(key.map(|key| (config::KEY_ENV.to_owned(), key.to_owned())));
        variables
    }

    fn read(&self, tally: &mut Tally, line: &str) {
        let Ok(event) = serde_json::from_str::<Value>(line) else { return };
        match event["type"].as_str() {
            Some("step_finish") => {
                let tokens = &event["part"]["tokens"];
                let count = |value: &Value| value.as_u64().unwrap_or(0);
                tally.spend(count(&tokens["input"]), count(&tokens["output"]) + count(&tokens["reasoning"]), count(&tokens["cache"]["read"]), count(&tokens["cache"]["write"]));
            }
            Some("text" | "tool_use") => tally.progressed = true,
            Some("error") => tally.error = Some(event["error"]["message"].as_str().or_else(|| event["error"]["type"].as_str()).unwrap_or("error").to_owned()),
            _ => {}
        }
    }

    fn entries(&self, event: &Value) -> Vec<(EntryKind, String)> {
        match event["type"].as_str() {
            Some("text") => vec![(EntryKind::Assistant, event["part"]["text"].as_str().unwrap_or_default().to_owned())],
            Some("tool_use") => vec![(EntryKind::Tool, tool_text(event["part"]["tool"].as_str().unwrap_or("tool"), &event["part"]["state"]["input"]))],
            Some("error") => vec![(EntryKind::Error, event["error"]["message"].as_str().unwrap_or("error").to_owned())],
            _ => Vec::new(),
        }
    }

    fn skills(&self) -> &'static str {
        "config/opencode/skills"
    }

    fn settings(&self) -> &'static str {
        "harness/opencode.toml"
    }

    fn slots_table(&self) -> &'static str {
        "go"
    }
}
