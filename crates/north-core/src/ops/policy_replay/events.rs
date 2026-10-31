use std::collections::BTreeMap;

use regex::Regex;
use serde::Serialize;

use super::json::Json;
use super::pyfmt::{round_int, round_to, splitlines, strip};
use crate::ops::advisor_report::stats::float_sum;

pub fn parse_events(stdout: &str) -> Vec<Json> {
    splitlines(stdout, false)
        .into_iter()
        .map(strip)
        .filter(|line| line.starts_with('{'))
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub fn final_text(evs: &[Json]) -> &str {
    evs.iter().rev().filter(|ev| ev.is_type("result")).find_map(|ev| ev.field("result").as_str()).unwrap_or("")
}

fn content_blocks(ev: &Json) -> &[Json] {
    ev.field("message").field("content").items()
}

pub fn tool_calls(evs: &[Json]) -> Vec<&Json> {
    evs.iter().filter(|ev| ev.is_type("assistant")).flat_map(content_blocks).filter(|block| block.is_type("tool_use")).collect()
}

fn main_model(evs: &[Json]) -> Option<&str> {
    evs.iter().filter(|ev| ev.is_type("system")).find_map(|ev| ev.nonempty("model"))
}

fn last_result(evs: &[Json]) -> Option<&Json> {
    evs.iter().rev().find(|ev| ev.is_type("result"))
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Usage {
    pub subtype: Option<String>,
    pub turns: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub model_resolved: Option<String>,
    pub cost_api_usd_comparison: f64,
    pub failure: Option<String>,
}

pub fn usage(evs: &[Json]) -> Option<Usage> {
    let ev = last_result(evs)?;
    let counts = ev.field("usage");
    let model = main_model(evs).or_else(|| ev.field("modelUsage").entries().first().map(|(name, _)| name.as_str()));
    let failure = ev.field("is_error").is_truthy().then(|| ev.nonempty("result").unwrap_or("result is_error with no text").to_owned());
    Some(Usage {
        subtype: ev.text("subtype").map(str::to_owned),
        turns: ev.field("num_turns").integer(),
        input_tokens: counts.field("input_tokens").integer(),
        output_tokens: counts.field("output_tokens").integer(),
        cache_creation_tokens: counts.field("cache_creation_input_tokens").integer(),
        cache_read_tokens: counts.field("cache_read_input_tokens").integer(),
        model_resolved: model.map(str::to_owned),
        cost_api_usd_comparison: round_to(ev.field("total_cost_usd").number(), 4),
        failure,
    })
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AdvisorUsage {
    pub advisor_calls: usize,
    pub advisor_model_resolved: Option<String>,
    pub advisor_errors: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisor_input_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisor_output_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisor_usd: Option<f64>,
}

fn advisor_call_count(evs: &[Json]) -> usize {
    let mut ids: Vec<String> = Vec::new();
    for block in evs.iter().filter(|ev| ev.is_type("assistant")).flat_map(content_blocks) {
        if block.is_type("server_tool_use") && block.text("name") == Some("advisor") {
            let id = block.field("id").dumps();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids.len()
}

fn advisor_errors(evs: &[Json]) -> BTreeMap<String, u64> {
    let mut errors = BTreeMap::new();
    for block in evs.iter().filter(|ev| ev.is_type("assistant")).flat_map(content_blocks) {
        let content = block.field("content");
        if block.is_type("advisor_tool_result") && content.is_type("advisor_tool_result_error") {
            let code = match content.field("error_code") {
                Json::String(text) => text.clone(),
                Json::Null => "None".to_owned(),
                other => other.dumps(),
            };
            *errors.entry(code).or_insert(0) += 1;
        }
    }
    errors
}

pub fn advisor_usage(evs: &[Json]) -> AdvisorUsage {
    let main = main_model(evs);
    let result = last_result(evs).unwrap_or(&Json::Null);
    let named = result.field("usage").field("iterations").items().iter().find(|i| i.is_type("advisor_message")).and_then(|i| i.nonempty("model"));
    let others: Vec<&(String, Json)> = result.field("modelUsage").entries().iter().filter(|(name, _)| Some(name.as_str()) != main).collect();
    let mut row = AdvisorUsage {
        advisor_calls: advisor_call_count(evs),
        advisor_model_resolved: named.or_else(|| others.first().map(|(name, _)| name.as_str())).map(str::to_owned),
        advisor_errors: advisor_errors(evs),
        advisor_input_tokens: None,
        advisor_output_tokens: None,
        advisor_usd: None,
    };
    if !others.is_empty() {
        let sum = |key: &str| others.iter().map(|(_, usage)| usage.field(key).integer()).sum::<i64>();
        row.advisor_input_tokens = Some(sum("inputTokens") + sum("cacheReadInputTokens") + sum("cacheCreationInputTokens"));
        row.advisor_output_tokens = Some(sum("outputTokens"));
        row.advisor_usd = Some(round_to(float_sum(others.iter().map(|(_, usage)| usage.field("costUSD").number())), 4));
    }
    row
}

#[derive(Debug, PartialEq, Serialize)]
pub struct AdvisorDebug {
    pub advisor_attached: bool,
    pub advisor_model_resolved: Option<String>,
}

pub fn advisor_debug(text: &str) -> AdvisorDebug {
    let pattern = Regex::new(r"\[AdvisorTool\] Server-side tool enabled with (\S+)").expect("advisor line pattern compiles");
    let enabled = pattern.captures_iter(text).last().map(|caps| caps[1].to_owned());
    AdvisorDebug { advisor_attached: enabled.is_some(), advisor_model_resolved: enabled }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Meter {
    pub five_hour_pct: Option<i64>,
    pub seven_day_pct: Option<i64>,
}

pub fn meter(evs: &[Json]) -> Meter {
    let windows = evs.iter().find(|ev| ev.is_type("rate_limit_event")).map(|ev| ev.field("rate_limit_info").field("unifiedWindows"));
    let pct = |name: &str| {
        let utilization = windows?.field(name).field("utilization");
        (*utilization != Json::Null).then(|| round_int(100.0 * utilization.number()))
    };
    Meter { five_hour_pct: pct("five_hour"), seven_day_pct: pct("seven_day") }
}
