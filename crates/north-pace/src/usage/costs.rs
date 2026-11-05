//! One line per non-AI service of the operator's costs.toml from its costs/<id>.json readout, then the weekly total,
//! and the short form `north usage` prints: the services still billing and the weekly total.
use std::fs;

use serde_json::Value;
use toml_edit::Table;

use super::{Places, py, settings};
use crate::foundation::clock::Timestamp;

pub(super) struct Costs {
    pub lines: Vec<String>,
    pub current: Vec<String>,
    pub fixed: f64,
    pub metered: f64,
}

pub(super) fn lines(places: &Places, now: Timestamp) -> Result<Vec<String>, String> {
    let Some(costs) = read(places, now)? else { return Ok(Vec::new()) };
    let mut lines = costs.lines;
    lines.push(format!("weekly operating ${:.2} = fixed ${:.2} + metered ${:.2}", costs.fixed + costs.metered, costs.fixed, costs.metered));
    Ok(lines)
}

pub(super) fn read(places: &Places, now: Timestamp) -> Result<Option<Costs>, String> {
    let Some(services) = settings::services(&places.operator) else { return Ok(None) };
    let readout = |id: &str| -> Option<Value> {
        let text = fs::read_to_string(places.state.join("agents/costs").join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    };
    let mut costs = Costs { lines: Vec::new(), current: Vec::new(), fixed: 0.0, metered: 0.0 };
    for service in &services {
        let (id, kind, billing) = (text(service, "id"), text(service, "kind"), text(service, "billing"));
        let shown = Shown { name: text_opt(service, "name").unwrap_or_else(|| id.clone()), current: !retired(service, now)? };
        if billing == "prepaid" && service.contains_key("initial") {
            let spent = readout(&id).and_then(|found| found["usd_spent_period"].as_f64());
            let (line, left) = prepaid(service, (id.as_str(), kind.as_str()), spent, now)?;
            costs.lines.push(line);
            shown.push(&mut costs.current, Some(format!("{left} left")));
            continue;
        }
        if kind == "ai-plan" {
            continue;
        }
        let Some(found) = readout(&id) else {
            costs.lines.push(format!("{id} ({kind}, {billing}): no readout yet"));
            shown.push(&mut costs.current, None);
            continue;
        };
        let spent = found["usd_spent_period"].as_f64().ok_or_else(|| format!("{id}: no usd_spent_period"))?;
        let (line, weekly) = metered(&found, (id.as_str(), kind.as_str(), billing.as_str()), spent, now)?;
        if billing != "fixed" {
            costs.metered += weekly;
        }
        costs.lines.push(line);
        shown.push(&mut costs.current, Some(format!("${spent:.2}")));
    }
    costs.fixed = services.iter().map(|service| number(service, "usd_per_week").unwrap_or(0.0)).sum();
    Ok(Some(costs))
}

struct Shown {
    name: String,
    current: bool,
}

impl Shown {
    fn push(self, current: &mut Vec<String>, amount: Option<String>) {
        if self.current {
            current.push(amount.map_or_else(|| self.name.clone(), |amount| format!("{} {amount}", self.name)));
        }
    }
}

fn retired(service: &Table, now: Timestamp) -> Result<bool, String> {
    let status = text_opt(service, "status").unwrap_or_default();
    if status.starts_with("deleted") || status == "deprecated" {
        return Ok(true);
    }
    Ok(match expiry(service) {
        Some(expires) => now >= py::parse(&expires)?,
        None => false,
    })
}

fn metered(found: &Value, (id, kind, billing): (&str, &str, &str), spent: f64, now: Timestamp) -> Result<(String, f64), String> {
    let moment = |key: &str| py::parse(found[key].as_str().unwrap_or_default());
    let (start, end) = (moment("period_start")?, moment("period_end")?);
    let read = found["read_at"].as_str().filter(|text| !text.is_empty()).map_or(Ok(now), py::parse)?;
    let days = (((std::cmp::min(read, end).unix_nanos() - start.unix_nanos()) / 1000) as f64 / 1e6 / 86400.0).max(1.0);
    let mut line = format!("{id} ({kind}, {billing}): ${spent:.2} spent {}..{}", py::day(start), py::day(end));
    if let Some(balance) = found["usd_balance"].as_f64() {
        line.push_str(&format!(", balance ${balance:.2}"));
    }
    if let Some(credits) = found["credits_left"].as_f64() {
        line.push_str(&format!(", credits left {}", py::g(credits)));
    }
    let source = found["source"].as_str().unwrap_or("?");
    let age = match found["read_at"].as_str().filter(|text| !text.is_empty()) {
        Some(read_at) => Some(py::minutes_precise(py::parse(read_at)?, now)),
        None => None,
    };
    let line = line + &format!(" ({source}") + &age.map_or(")".to_owned(), |age| format!(", {age:.0} min ago)"));
    Ok((line, spent * 7.0 / days))
}

fn expiry(service: &Table) -> Option<String> {
    service.get("expires").and_then(|item| item.as_datetime()).map(|moment| moment.to_string()).or_else(|| text_opt(service, "expires"))
}

fn prepaid(service: &Table, (id, kind): (&str, &str), spent: Option<f64>, now: Timestamp) -> Result<(String, String), String> {
    let expires = py::parse(&expiry(service).ok_or_else(|| format!("{id}: no expires"))?)?;
    let initial = number(service, "initial").unwrap_or_default();
    let left = if now >= expires {
        "$0.00".to_owned()
    } else {
        spent.map_or("no readout, unknown".to_owned(), |spent| format!("${:.2}", (initial - spent).max(0.0)))
    };
    let readout = text_opt(service, "readout").unwrap_or_else(|| "?".to_owned());
    let line = format!("{id} ({kind}, prepaid): {left} left of ${initial:.2}, expires {} (readout: {readout})", py::minute(expires));
    Ok((line, if spent.is_some() { left } else { "unknown".to_owned() }))
}

fn text(table: &Table, key: &str) -> String {
    text_opt(table, key).unwrap_or_else(|| "?".to_owned())
}

fn text_opt(table: &Table, key: &str) -> Option<String> {
    table.get(key).and_then(|item| item.as_str()).map(str::to_owned)
}

fn number(table: &Table, key: &str) -> Option<f64> {
    let item = table.get(key)?;
    item.as_float().or_else(|| item.as_integer().map(|integer| integer as f64))
}
