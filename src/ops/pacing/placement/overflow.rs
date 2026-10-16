use super::super::storage::rows;
use super::moment;
use serde_json::Value;
use toml_edit::{DocumentMut, Item};

type Reading = (f64, f64, f64);

pub(in crate::ops::pacing) fn overflow(cfg: &DocumentMut, probe: &Value, at: f64) -> Option<String> {
    let rules = cfg.get("placement");
    let target = rules
        .and_then(|p| p.get("claude_session_target"))
        .and_then(Item::as_integer)
        .unwrap_or(90) as f64;
    let ceiling = rules
        .and_then(|p| p.get("claude_session_ceiling"))
        .and_then(Item::as_integer)
        .unwrap_or(95) as f64;
    if moment(&probe["claude_throttled"]).is_some_and(|t| (0.0..=900.0).contains(&(at - t))) {
        return Some(format!(
            "claude:a 5h 429 {:.0} min ago",
            (at - moment(&probe["claude_throttled"]).unwrap()) / 60.0
        ));
    }
    session_overflow(probe, at, target, ceiling).or_else(|| pace_overflow(probe))
}
fn readings(probe: &Value, at: f64) -> Vec<Reading> {
    rows(&probe["claude_rows"])
        .iter()
        .filter_map(|r| {
            Some((
                moment(&r["ts"])?,
                r["used_pct"].as_f64()?,
                moment(&r["resets_at"])?,
            ))
        })
        .filter(|r| r.0 <= at)
        .collect()
}
fn session_overflow(probe: &Value, at: f64, target: f64, ceiling: f64) -> Option<String> {
    let mut readings = readings(probe, at);
    let reset = readings
        .iter()
        .map(|r| r.2)
        .max_by(f64::total_cmp)
        .filter(|t| *t > at)?;
    readings.retain(|r| reset - r.2 <= 300.0);
    readings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let last = readings.last().unwrap();
    let first = readings.iter().find(|r| last.0 - r.0 <= 1800.0).unwrap();
    let burn = if last.0 > first.0 {
        ((last.1 - first.1) / (last.0 - first.0)).max(0.0)
    } else {
        0.0
    };
    let projected = last.1 + burn * (reset - last.0);
    if last.1 >= ceiling {
        return Some(format!("claude:a 5h {:.0}% >= {ceiling}% ceiling", last.1));
    }
    if last.1 >= target {
        return Some(format!("claude:a 5h {:.0}% >= {target}%", last.1));
    }
    if projected > target {
        return Some(format!("claude:a 5h projected {projected:.0}% > {target}%"));
    }
    None
}
fn pace_overflow(probe: &Value) -> Option<String> {
    let paces = probe["claude_paces"].as_object()?;
    if paces.is_empty()
        || !paces
            .values()
            .all(|p| p["state"] == "at or over pace" || p["stale"] == true)
    {
        return None;
    }
    Some(format!(
        "no Claude account admits: {}",
        paces
            .keys()
            .map(|n| format!("{n} at or over pace"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
