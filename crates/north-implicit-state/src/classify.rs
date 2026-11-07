use crate::presence::World;
use crate::shape::Shape;
use crate::{Config, Finding, Source, status, structs};
use north_base::platform::ast_grep::Match;

/// Each finding in one source's matches; a source under a lifecycle owner keeps its state its own way.
pub(crate) fn findings(config: &Config, source: &Source, matches: &[Match], world: &World) -> Vec<Finding> {
    if config.owns(source.path) {
        return Vec::new();
    }
    let unit_file = source.text.contains("\"systemctl\"");
    let mut out = Vec::new();
    for found in matches {
        let finding = |shape: Shape, item: String| Finding { shape, path: source.path.to_owned(), line: found.line, item };
        match found.rule.as_str() {
            "status-sql" => out.extend(sql(source, found)),
            "state-struct" => out.extend(lifecycle(source, found)),
            "presence-check" | "presence-read" => out.extend(world.flag(source, found).map(|anchor| finding(Shape::PresenceFlag, anchor))),
            "unit-intent" if !unit_file => {}
            rule => out.extend(simple(rule, found).map(|(shape, item)| finding(shape, item))),
        }
    }
    out
}

fn simple(rule: &str, found: &Match) -> Option<(Shape, String)> {
    let text = normal(&found.text);
    let shape = match rule {
        "marker-stamp" if text.contains("now") || text.contains("\"touch\"") || text.contains("Date()") => Shape::MarkerStamp,
        "pid-liveness" => Shape::PidLiveness,
        "phase-suffix" => Shape::PhaseSuffix,
        "unit-intent" => Shape::UnitIntent,
        "mtime-read" => Shape::MtimeRead,
        "unit-read" => Shape::UnitRead,
        "link-read" => Shape::LinkRead,
        "lock-flag" => Shape::LockFlag,
        "status-compare" => return status::compared(found.single.get("L")?, found.single.get("R")?).map(|item| (Shape::StatusString, item)),
        "status-prefix" => return status::status_name(found.single.get("L")?).map(|_| (Shape::StatusString, text)),
        "status-match" => return status::status_name(found.single.get("L")?).map(|_| (Shape::StatusString, format!("match {}", normal(&found.single["L"])))),
        "status-matches" => return status::matches_macro(&text).map(|item| (Shape::StatusString, format!("matches!({item})"))),
        _ => return None,
    };
    Some((shape, text))
}

fn sql(source: &Source, found: &Match) -> Vec<Finding> {
    status::sql(&found.text)
        .into_iter()
        .map(|(offset, item)| Finding { shape: Shape::StatusString, path: source.path.to_owned(), line: found.line + found.text[..offset].matches('\n').count(), item })
        .collect()
}

fn lifecycle(source: &Source, found: &Match) -> Vec<Finding> {
    let Some(name) = found.single.get("N") else { return Vec::new() };
    let fields = structs::fields(&found.text);
    let finding = |shape: Shape, item: String| Finding { shape, path: source.path.to_owned(), line: found.line, item };
    let mut out = Vec::new();
    if fields.lifecycle.len() >= 2 {
        out.push(finding(Shape::LifecycleBools, format!("{name} {{ {} }}", fields.lifecycle.join(", "))));
    }
    if !fields.lifecycle.is_empty() && fields.lifecycle.len() + fields.optional >= 4 {
        out.push(finding(Shape::StateEntropy, format!("{name}: {} lifecycle booleans, {} optional fields", fields.lifecycle.len(), fields.optional)));
    }
    out
}

pub(crate) fn normal(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
