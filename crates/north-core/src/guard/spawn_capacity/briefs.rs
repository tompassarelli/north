use super::{LOCAL_CLAUDE_TYPES, World, capacity_home, logs};
use crate::hook::HookInput;
use crate::platform::pyjson;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub(super) static LANDING_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Landing-path:\s*yes\b").unwrap());
static CLOSING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Closing:\s*yes\b").unwrap());
static LANDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\blander\b").unwrap());
pub(super) static URGENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?mi)^\W*CASE=URGENT\s+FACT=(?:["“”]([^"“”]*)["“”]|['‘’]([^'‘’]*)['‘’])"#).unwrap());
static URGENT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bCASE=URGENT\b").unwrap());
static LANDING_WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:Autoland|safe-push|landing[ -]queue|main[ -]red)\b").unwrap());
static RESEARCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Category:\s*research\b").unwrap());
static LANE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"code/([A-Za-z0-9][A-Za-z0-9_.-]*)/worktrees/").unwrap());

pub struct Event {
    pub raw: Value,
    pub texts: Vec<String>,
}

impl Event {
    pub fn from_hook(input: &HookInput) -> Option<Self> {
        let raw: Value = serde_json::from_str(&pyjson::dumps(&input.data)).ok()?;
        raw.as_object()?;
        let texts = input.data.get("tool_input").filter(|input| py_truthy(input)).map(brief_texts).unwrap_or_default();
        Some(Self { raw, texts })
    }

    fn tool(&self) -> &str {
        self.raw["tool_name"].as_str().unwrap_or("")
    }

    pub(super) fn provider(&self) -> &'static str {
        if self.raw["tool_name"] == "Agent" { "claude" } else { "codex" }
    }

    pub(super) fn session(&self) -> Value {
        if truthy(&self.raw["agent_id"]) { Value::Null } else { self.raw["session_id"].clone() }
    }

    fn input(&self) -> Option<&serde_json::Map<String, Value>> {
        self.raw["tool_input"].as_object()
    }

    pub(super) fn task(&self) -> Value {
        let Some(input) = self.input() else { return Value::Null };
        let task = input.get("task_name").cloned().unwrap_or(Value::Null);
        if truthy(&task) { task } else { input.get("name").cloned().unwrap_or(Value::Null) }
    }

    fn subagent_local(&self) -> bool {
        let kind = self.input().and_then(|input| input.get("subagent_type")).filter(|kind| truthy(kind));
        kind.map_or(Some("general-purpose"), Value::as_str).is_some_and(|kind| LOCAL_CLAUDE_TYPES.contains(&kind))
    }

    pub(super) fn is_local(&self) -> bool {
        if self.tool().ends_with("spawn_agent") {
            return true;
        }
        if self.raw["tool_name"] != "Agent" || self.input().is_none() {
            return false;
        }
        self.raw["tool_input"]["isolation"] != "remote" && self.subagent_local()
    }

    pub(super) fn named_lander(&self) -> bool {
        self.input().and_then(|input| input.get("name")).filter(|name| truthy(name)).is_some_and(|name| LANDER.is_match(&text(name)))
    }
}

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
    }
}

pub(super) fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn py_truthy(value: &pyjson::Value) -> bool {
    match value {
        pyjson::Value::Null => false,
        pyjson::Value::Bool(flag) => *flag,
        pyjson::Value::Number { raw, .. } => raw.parse::<f64>() != Ok(0.0),
        pyjson::Value::String(text) => !text.codepoints.is_empty(),
        pyjson::Value::Array(items) => !items.is_empty(),
        pyjson::Value::Object(items) => !items.is_empty(),
    }
}

fn brief_texts(value: &pyjson::Value) -> Vec<String> {
    match value {
        pyjson::Value::String(raw) => match pyjson::loads_string(raw) {
            Ok(pyjson::Value::String(_)) | Err(_) => vec![raw.lossy().into_owned()],
            Ok(parsed) => brief_texts(&parsed),
        },
        pyjson::Value::Object(items) => items.iter().flat_map(|(_, value)| brief_texts(value)).collect(),
        pyjson::Value::Array(items) => items.iter().flat_map(brief_texts).collect(),
        _ => Vec::new(),
    }
}

pub(super) fn urgent_file_fact(world: &impl World, texts: &[String]) -> Option<String> {
    let path = capacity_home(world).join(".local/state/agents/spawn-urgent.tsv");
    let lines = std::fs::read_to_string(path).ok()?;
    let names: Vec<&str> = texts.iter().filter(|text| text.chars().count() <= 80).map(|text| text.trim()).collect();
    let now = world.now();
    lines.lines().rev().find_map(|line| {
        let mut parts = line.splitn(3, '\t');
        let (epoch, name, fact) = (parts.next()?, parts.next()?, parts.next()?);
        let fresh = !epoch.is_empty() && epoch.bytes().all(|b| b.is_ascii_digit()) && now - epoch.parse::<f64>().ok()? <= 900.0;
        (fresh && names.contains(&name) && fact.trim().chars().count() >= 15).then(|| fact.trim().to_owned())
    })
}

fn lane_repository(event: &Event) -> Option<String> {
    let cwd = event.raw["cwd"].as_str().filter(|cwd| !cwd.is_empty()).map(str::to_owned);
    let found = |source: &str| LANE.captures(&format!("{}/", source.trim_end_matches('/'))).map(|found| found[1].to_owned());
    event.texts.iter().find_map(|text| found(text)).or_else(|| found(cwd.as_deref().unwrap_or("")))
}

pub(super) fn flow_gate(world: &impl World, event: &Event) -> Option<String> {
    if event.raw["tool_name"] == "Agent" && event.input().is_some() && !event.subagent_local() {
        return None;
    }
    let texts = &event.texts;
    if texts.iter().any(|text| URGENT_WORD.is_match(text)) || urgent_file_fact(world, texts).is_some() {
        return None;
    }
    if texts.iter().any(|text| LANDING_WORDS.is_match(text)) {
        return None;
    }
    if texts.iter().any(|text| LANDING_PATH.is_match(text) || CLOSING.is_match(text)) || event.named_lander() {
        return None;
    }
    if texts.iter().any(|text| RESEARCH.is_match(text)) {
        return None;
    }
    let repo = lane_repository(event)?;
    let flow: Value = serde_json::from_str(&world.flow(&repo)?).ok()?;
    let (pending, rate) = (flow["row"]["pending"].as_f64()?, flow["row"]["landed_per_hour"].as_f64()?);
    flow.get("line")?;
    if pending <= rate.max(2.0) {
        return None;
    }
    let reason = format!("{}. Refusing a new code worker: widen the landing path first. A closing pass that pushes no lane passes with a `Closing: yes` brief line.", text(&flow["line"]));
    logs::denial(world, event, "flow", Some(&reason));
    Some(reason)
}

pub(super) fn py_g(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if !value.is_finite() {
        return if value.is_nan() { "nan".into() } else if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..6).contains(&exponent) {
        let mantissa = trim_zeros(mantissa);
        return format!("{mantissa}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    trim_zeros(&format!("{value:.decimals$}")).to_owned()
}

fn trim_zeros(number: &str) -> &str {
    if number.contains('.') { number.trim_end_matches('0').trim_end_matches('.') } else { number }
}
