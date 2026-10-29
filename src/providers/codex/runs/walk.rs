use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::providers::claude::runs::truthy;
use crate::providers::ledger_rows::{self as rows, Shared};

static BRIEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bItem:\s*(?:[\w-]+/)?[\w-]+#\d+").unwrap());
static CODE_REPO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/code/([\w.-]+)/(?:main|worktrees)\b").unwrap());
static COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bcmd"?\s*:\s*("(?:[^"\\]|\\.)*")"#).unwrap());
static EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"exit_code\W+(-?\d+)").unwrap());

type Step = (String, Option<i64>, Option<String>, Option<String>);

pub(super) struct Walk {
    pub id: String,
    pub parent: Option<String>,
    agent_path: String,
    pub brief: String,
    pub report: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub staffed: Option<String>,
    pub review: Option<String>,
    pub final_message: bool,
    pub tokens: i64,
    pub steps: Vec<Step>,
    pub repos: BTreeSet<String>,
    pub records: Vec<Value>,
}

pub(crate) fn records(path: &Path) -> Vec<Value> {
    let text = fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(Value::is_object).collect()
}

fn texts(payload: &Value) -> String {
    match &payload["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter(|part| part.is_object()).map(|part| part["text"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn plain(value: &Value) -> String {
    value.as_str().map_or_else(|| value.to_string(), str::to_owned)
}

fn decoded(output: &Value) -> String {
    let items: Vec<&Value> = match output {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    items
        .into_iter()
        .map(|item| {
            let inner = if item.is_object() { item.get("text").unwrap_or(item) } else { item };
            let value = inner.as_str().and_then(|text| serde_json::from_str::<Value>(text).ok()).unwrap_or_else(|| inner.clone());
            match value.as_object() {
                Some(object) => object.get("output").map_or_else(String::new, plain),
                None => plain(&value),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_field(value: &Value) -> Option<String> {
    value.as_str().filter(|text| !text.is_empty()).map(str::to_owned)
}

impl Walk {
    pub(super) fn read(
        path: &Path,
        shared: &Shared,
        models: &HashMap<String, (Option<String>, Option<String>)>,
        briefs: &mut impl FnMut(&str) -> HashMap<String, Value>,
    ) -> Option<Self> {
        let mut walk = Self {
            id: String::new(),
            parent: None,
            agent_path: String::new(),
            brief: String::new(),
            report: String::new(),
            model: None,
            effort: None,
            started: None,
            ended: None,
            staffed: None,
            review: None,
            final_message: false,
            tokens: 0,
            steps: Vec::new(),
            repos: BTreeSet::new(),
            records: records(path),
        };
        let mut cells: HashMap<String, Vec<usize>> = HashMap::new();
        let mut seen_meta = false;
        let records = std::mem::take(&mut walk.records);
        for record in &records {
            let payload = &record["payload"];
            let kind = record["type"].as_str().unwrap_or("");
            if kind == "session_meta" && !seen_meta {
                seen_meta = true;
                if !walk.open(payload, shared, models, briefs) {
                    return None;
                }
            }
            let Some(started) = walk.started.as_deref() else { continue };
            let at = record["timestamp"].as_str();
            if at.unwrap_or("") < started {
                continue;
            }
            walk.ended = at.filter(|at| !at.is_empty()).map(str::to_owned).or(walk.ended.take());
            match kind {
                "turn_context" => {
                    walk.model = text_field(&payload["model"]).or(walk.model.take());
                    walk.effort = text_field(&payload["effort"]).or(walk.effort.take());
                }
                "response_item" => walk.response(payload, at, &mut cells),
                "event_msg" => walk.event(payload, at),
                _ => {}
            }
        }
        walk.records = records;
        seen_meta.then_some(walk)
    }

    fn open(&mut self, payload: &Value, shared: &Shared, models: &HashMap<String, (Option<String>, Option<String>)>, briefs: &mut impl FnMut(&str) -> HashMap<String, Value>) -> bool {
        let spawn = &payload["source"]["subagent"]["thread_spawn"];
        self.id = payload["id"].as_str().unwrap_or("").to_owned();
        self.parent = text_field(&spawn["parent_thread_id"]).or_else(|| text_field(&payload["parent_thread_id"]));
        if shared.known.contains(&self.id) || (!shared.session.is_empty() && self.parent.as_deref() != Some(shared.session) && self.id != shared.session) {
            return false;
        }
        self.agent_path = text_field(&spawn["agent_path"]).or_else(|| text_field(&payload["agent_path"])).unwrap_or_default();
        let assignment = self.parent.as_deref().map(briefs).and_then(|named| named.get(&self.agent_path).cloned()).unwrap_or(Value::Null);
        self.brief = assignment["message"].as_str().unwrap_or("").to_owned();
        let known = models.get(&self.id).cloned().unwrap_or((None, None));
        self.model = text_field(&assignment["model"]).or(known.0);
        self.effort = text_field(&assignment["reasoning_effort"]).or(known.1);
        let started = payload["timestamp"].as_str().unwrap_or("").to_owned();
        self.staffed = text_field(&assignment["staffed"]).or_else(|| Some(started.clone()));
        self.started = Some(started);
        true
    }

    fn response(&mut self, payload: &Value, at: Option<&str>, cells: &mut HashMap<String, Vec<usize>>) {
        let kind = payload["type"].as_str().unwrap_or("");
        let role = payload["role"].as_str().unwrap_or("");
        match kind {
            "message" if role == "user" || role == "assistant" => {
                let text = texts(payload);
                if BRIEF.is_match(&text) {
                    self.brief = text.clone();
                }
                if role == "user" {
                    self.final_message = false;
                } else {
                    self.final_message = payload["phase"] == "final";
                    if self.final_message {
                        self.report = text;
                        self.review = self.review.take().or(at.map(str::to_owned));
                    }
                }
            }
            "function_call" | "custom_tool_call" => self.call(payload, cells),
            "function_call_output" | "custom_tool_call_output" => self.output(payload, at, cells),
            _ => {}
        }
    }

    fn call(&mut self, payload: &Value, cells: &mut HashMap<String, Vec<usize>>) {
        self.final_message = false;
        let arguments = if truthy(&payload["arguments"]) { &payload["arguments"] } else { &payload["input"] };
        let args = match arguments {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        self.repos.extend(CODE_REPO.captures_iter(&args).map(|found| found[1].to_owned()));
        let commands: Vec<String> = COMMAND.captures_iter(&args).filter_map(|found| serde_json::from_str::<String>(&found[1]).ok()).collect();
        let first = self.steps.len();
        cells.insert(payload["call_id"].as_str().unwrap_or("").to_owned(), (first..first + commands.len()).collect());
        self.steps.extend(commands.into_iter().map(|command| (command, None, None, None)));
    }

    fn output(&mut self, payload: &Value, at: Option<&str>, cells: &mut HashMap<String, Vec<usize>>) {
        let Some(indices) = payload["call_id"].as_str().and_then(|call| cells.remove(call)) else { return };
        let dumped = payload["output"].to_string();
        let codes: Vec<i64> = EXIT.captures_iter(&dumped).filter_map(|found| found[1].parse().ok()).collect();
        let output = decoded(&payload["output"]);
        for (index, code) in indices.into_iter().zip(codes) {
            self.steps[index].1 = Some(code);
            self.steps[index].2 = Some(output.clone());
            self.steps[index].3 = at.map(str::to_owned);
        }
    }

    fn event(&mut self, payload: &Value, at: Option<&str>) {
        match payload["type"].as_str().unwrap_or("") {
            "task_started" | "user_message" => self.final_message = false,
            "task_complete" => {
                self.final_message = true;
                self.review = self.review.take().or(at.map(str::to_owned));
                self.report = text_field(&payload["last_agent_message"]).unwrap_or_else(|| std::mem::take(&mut self.report));
            }
            "token_count" if truthy(&payload["info"]) => {
                let total = &payload["info"]["total_token_usage"]["output_tokens"];
                self.tokens = total.as_i64().unwrap_or(self.tokens);
            }
            _ => {}
        }
    }

    pub(super) fn name(&self) -> &str {
        self.agent_path.rsplit('/').next().unwrap_or("")
    }

    pub(super) fn finished(&self, shared: &Shared, cutoff: f64) -> bool {
        let ended = self.ended.as_deref().and_then(rows::stamp);
        !shared.known.contains(&self.id) && ended.is_some_and(|ended| ended >= cutoff) && self.model.is_some() && self.effort.is_some()
    }
}
