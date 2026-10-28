//! One line per non-AI service of the operator's costs.toml from its costs/<id>.json readout, then the weekly total.
use std::fs;

use serde_json::Value;
use toml_edit::Table;

use super::{Places, py, settings};
use crate::foundation::clock::Timestamp;

pub(super) fn lines(places: &Places, now: Timestamp) -> Result<Vec<String>, String> {
    let Some(services) = settings::services(&places.operator) else { return Ok(Vec::new()) };
    let readout = |id: &str| -> Option<Value> {
        let text = fs::read_to_string(places.state.join("agents/costs").join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    };
    let (mut lines, mut metered) = (Vec::new(), 0.0_f64);
    for service in &services {
        let (id, kind, billing) = (text(service, "id"), text(service, "kind"), text(service, "billing"));
        if billing == "prepaid" && service.contains_key("initial") {
            let spent = readout(&id).and_then(|found| found["usd_spent_period"].as_f64());
            lines.push(prepaid(service, (id.as_str(), kind.as_str()), spent, now)?);
            continue;
        }
        if kind == "ai-plan" {
            continue;
        }
        let Some(found) = readout(&id) else {
            lines.push(format!("{id} ({kind}, {billing}): no readout yet"));
            continue;
        };
        let moment = |key: &str| py::parse(found[key].as_str().unwrap_or_default());
        let (start, end) = (moment("period_start")?, moment("period_end")?);
        let read = found["read_at"].as_str().filter(|text| !text.is_empty()).map_or(Ok(now), py::parse)?;
        let days = (((std::cmp::min(read, end).unix_nanos() - start.unix_nanos()) / 1000) as f64 / 1e6 / 86400.0).max(1.0);
        let spent = found["usd_spent_period"].as_f64().ok_or_else(|| format!("{id}: no usd_spent_period"))?;
        if billing != "fixed" {
            metered += spent * 7.0 / days;
        }
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
        lines.push(line + &format!(" ({source}") + &age.map_or(")".to_owned(), |age| format!(", {age:.0} min ago)")));
    }
    let fixed: f64 = services.iter().map(|service| number(service, "usd_per_week").unwrap_or(0.0)).sum();
    lines.push(format!("weekly operating ${:.2} = fixed ${fixed:.2} + metered ${metered:.2}", fixed + metered));
    Ok(lines)
}

fn prepaid(service: &Table, (id, kind): (&str, &str), spent: Option<f64>, now: Timestamp) -> Result<String, String> {
    let expires = service.get("expires").and_then(|item| item.as_datetime()).map(|moment| moment.to_string()).or_else(|| text_opt(service, "expires"));
    let expires = py::parse(&expires.ok_or_else(|| format!("{id}: no expires"))?)?;
    let initial = number(service, "initial").unwrap_or_default();
    let left = if now >= expires {
        "$0.00".to_owned()
    } else {
        spent.map_or("no readout, unknown".to_owned(), |spent| format!("${:.2}", (initial - spent).max(0.0)))
    };
    let readout = text_opt(service, "readout").unwrap_or_else(|| "?".to_owned());
    Ok(format!("{id} ({kind}, prepaid): {left} left of ${initial:.2}, expires {} (readout: {readout})", py::minute(expires)))
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
