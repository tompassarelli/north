use super::super::storage;
use super::{facets, field, strings};
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

pub(in crate::ops::pacing) fn routing_defaults() -> crate::router::policy::Defaults {
    let doc: DocumentMut = include_str!("../../../../../../harness/defaults.toml")
        .parse()
        .unwrap();
    let mut routes = BTreeMap::new();
    let mut tiers = BTreeMap::new();
    for (key, value) in doc.iter() {
        if let Some(category) = key.strip_prefix("codex_route_")
            && let Some(route) = value["value"].as_str() {
                routes.insert(category.into(), route.into());
            }
        if let Some(category) = key.strip_prefix("codex_tier_") {
            tiers.insert(category.into(), strings(value.get("value")));
        }
    }
    crate::router::policy::Defaults {
        codex_route: routes,
        codex_tier: tiers,
        codex_eta_max_min: storage::default("codex_eta_max_min") as u64,
        codex_files_max: storage::default("codex_files_max") as u64,
        placement_drain_margin: crate::pace::limits::placement_drain_margin(),
        reservation_fallback_points: storage::default("reservation_fallback_points"),
    }
}
pub(in crate::ops::pacing) fn tier_for(
    text: &str,
    cfg: &DocumentMut,
    provider: &str,
    floor: f64,
    difficulty: f64,
    defaults: &crate::router::policy::Defaults,
) -> Option<String> {
    let (_, _, needs) = facets(text);
    if provider == "gemini" && (needs.contains("private") || needs.contains("local-state")) {
        return None;
    }
    let full = tier_starts(cfg.get("tiers")?.as_table_like()?);
    let qualifies = crate::router::brief::codex_qualifies(text, false, defaults).0;
    let category = field(text, "Category");
    let class = crate::router::brief::codex_class(Some(category), defaults);
    let available = available_bands(&full, provider, floor);
    for (index, (key, lo)) in available.iter().enumerate() {
        if provider == "codex" && (!qualifies || !class.tiers.contains(key)) {
            continue;
        }
        let hi = available
            .get(index + 1)
            .map(|(_, s)| *s)
            .unwrap_or(f64::INFINITY);
        let success = cfg
            .get("success")
            .and_then(|s| s.get(format!("{key}|{category}")))
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)));
        if success.map_or(
            (index == 0 || difficulty >= *lo) && difficulty < hi,
            |rate| rate >= storage::default("success_threshold"),
        ) {
            return Some(key.clone());
        }
    }
    None
}
fn tier_starts(tiers: &dyn toml_edit::TableLike) -> Vec<(String, f64)> {
    let mut full = Vec::new();
    let mut previous = 0.0;
    for (index, (key, tier)) in tiers.iter().enumerate() {
        let start = if index == 0 {
            0.0
        } else {
            tier.get("start")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                .unwrap_or(0.0)
                .max(previous)
        };
        full.push((key.to_owned(), start));
        previous = start;
    }
    full
}
fn available_bands(full: &[(String, f64)], provider: &str, floor: f64) -> Vec<(String, f64)> {
    let mut bands: Vec<(String, f64, f64)> = Vec::new();
    let mut pending = None;
    for (index, (key, start)) in full.iter().enumerate() {
        let hi = full
            .get(index + 1)
            .map(|(_, start)| *start)
            .unwrap_or(100.0);
        if key.split(':').next() == Some(provider) {
            bands.push((key.clone(), pending.take().unwrap_or(*start), hi));
        } else if pending.is_none() {
            pending = Some(*start);
        }
    }
    if pending.is_some()
        && let Some(last) = bands.last_mut() {
            last.2 = 100.0;
        }
    bands
        .into_iter()
        .filter(|(_, lo, hi)| hi > lo && *hi > floor)
        .map(|(key, lo, _)| (key, lo))
        .collect()
}
