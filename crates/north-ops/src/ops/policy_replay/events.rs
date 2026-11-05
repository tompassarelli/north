use serde::Serialize;

use super::json::Json;
use super::pyfmt::{round_int, round_to, splitlines, strip};

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
