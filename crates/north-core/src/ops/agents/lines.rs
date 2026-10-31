use serde_json::{Map, Value, json};

use crate::router::ctx::Knobs;

pub(super) fn ask(op: &str, fields: Value) -> Result<Value, String> {
    let mut request = fields;
    request["op"] = json!(op);
    crate::router::ops::answer(&request).unwrap_or_else(|| Err(format!("unknown op {op}"))).map_err(|error| format!("north pace: {error}"))
}

pub(super) fn text(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(text) => text.clone(),
        Value::Number(number) if number.is_f64() => format!("{:?}", number.as_f64().unwrap_or_default()),
        other => other.to_string(),
    }
}

pub(super) fn whole(number: f64) -> String {
    if number.fract() == 0.0 { format!("{number:.0}") } else { number.to_string() }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().map(text).collect()
}

fn project_line(name: &str, project: &Value) -> String {
    let field = |key: &str| project.get(key).filter(|value| !value.is_null());
    let urgency = match field("target") {
        None => "no target".to_owned(),
        Some(target) => match project.get("u") {
            Some(u) if !field("unmeasured").is_some_and(truthy) => {
                format!("target {}, urgency {:.2}", text(target), u.as_f64().unwrap_or_default())
            }
            _ => format!("target {}, urgency unmeasured", text(target)),
        },
    };
    let share = field("share").map_or(String::new(), |share| format!(", share {}%", text(share)));
    let priority = field("priority").map_or("-".to_owned(), text);
    format!("project {name}: priority {priority}{share}, {urgency}")
}

fn truthy(value: &Value) -> bool {
    crate::router::ctx::truthy(value)
}

fn measured_lines(ctx: &Value) -> Vec<String> {
    let min_runs = Knobs::default().min_runs;
    let success = ctx.get("success").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut lines = Vec::new();
    for tier in ctx["tiers"].as_array().into_iter().flatten() {
        let key = tier["key"].as_str().unwrap_or_default();
        let mut rates: Vec<(&str, &Value)> = success
            .iter()
            .filter_map(|(cell, rate)| cell.split_once('|').filter(|(owner, _)| *owner == key).map(|(_, category)| (category, rate)))
            .collect();
        rates.sort_by(|left, right| left.0.cmp(right.0));
        let successor = tier.get("successor").filter(|value| truthy(value));
        if tier.get("runs").is_none() && rates.is_empty() && successor.is_none() {
            continue;
        }
        let runs = tier.get("runs").map_or("unknown".to_owned(), text);
        let mut line = format!("measured {key}: {runs} runs");
        if rates.is_empty() {
            line.push_str(&format!("; no category with {} runs", whole(min_runs)));
        } else {
            let shown: Vec<String> = rates.iter().map(|(category, rate)| format!("{category} {}%", text(rate))).collect();
            line.push_str(&format!("; success {}", shown.join(", ")));
        }
        if let Some(next) = successor {
            let model = next.get("model").map_or("unnamed".to_owned(), text);
            let days = next["eta_days"].as_f64().unwrap_or_default();
            let p = next["p"].as_f64().unwrap_or_default();
            line.push_str(&format!("; successor {model} in {days:.1} days at p {p:.2}"));
        }
        lines.push(line);
    }
    lines
}

pub(super) fn plan_lines(ctx: &Value, result: &Value) -> Result<Vec<String>, String> {
    let shown = ask("plan_lines", json!({"ctx": ctx, "result": result}))?;
    let empty = Map::new();
    let mut projects: Vec<(&String, &Value)> = ctx.get("projects").and_then(Value::as_object).unwrap_or(&empty).iter().collect();
    projects.sort_by(|left, right| {
        let priority = |project: &Value| project.get("priority").and_then(Value::as_f64).unwrap_or(f64::INFINITY);
        priority(left.1).total_cmp(&priority(right.1))
    });
    let mut lines = strings(&shown["head"]);
    lines.extend(projects.into_iter().map(|(name, project)| project_line(name, project)));
    lines.extend(measured_lines(ctx));
    lines.extend(strings(&shown["tail"]));
    Ok(lines)
}
