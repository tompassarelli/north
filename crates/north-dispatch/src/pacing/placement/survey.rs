use super::super::storage::rows;
use super::{fresh, minimum, moment, queued, release};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item, TableLike};

#[derive(Default)]
pub(super) struct Survey<'a> {
    pub(super) reasons: BTreeMap<String, String>,
    pub(super) controls: BTreeMap<String, &'a Value>,
    pub(super) etas: Vec<f64>,
}

fn auto_route_off(policy: Option<&Item>) -> bool {
    policy
        .and_then(|s| s.get("auto_route"))
        .and_then(Item::as_bool)
        == Some(false)
}

fn control_skipped(control: &Value, policy: Option<&Item>) -> bool {
    control["auto_route"] == false || control["auto_route"].is_null() && auto_route_off(policy)
}

fn control_reason<'a>(
    cfg: &DocumentMut,
    provider: &str,
    name: &str,
    control: &'a Value,
    at: f64,
) -> &'a str {
    let billing = cfg
        .get("billing")
        .and_then(|b| b.get(name).or_else(|| b.get(provider)))
        .and_then(|b| b.get("standard"))
        .and_then(Item::as_str)
        .unwrap_or("plan");
    if billing != "plan" {
        "blocked"
    } else if !fresh(control, at) || control.get("held_reason").is_none() {
        "stale"
    } else {
        control["held_reason"].as_str().unwrap_or_default()
    }
}

fn survey_provider<'a>(
    survey: &mut Survey<'a>,
    cfg: &DocumentMut,
    gate: &'a Value,
    provider: &str,
    at: f64,
) {
    let policy = cfg.get("accounts").and_then(|a| a.get(provider));
    let controls = gate[provider]["controls"].as_object();
    if controls.is_none_or(|c| c.is_empty()) && !auto_route_off(policy) {
        survey
            .reasons
            .insert(provider.to_owned(), "stale".to_owned());
        survey.etas.push(at + 60.0);
    }
    for (name, control) in controls.into_iter().flatten() {
        if control_skipped(control, policy) {
            continue;
        }
        let reason = control_reason(cfg, provider, name, control, at);
        survey.etas.extend(release(reason, control, at));
        survey.reasons.insert(name.clone(), reason.to_owned());
        survey.controls.insert(name.clone(), control);
    }
}

pub(super) fn survey_controls<'a>(
    cfg: &DocumentMut,
    gate: &'a Value,
    tiers: Option<&dyn TableLike>,
    at: f64,
) -> Survey<'a> {
    let mode = cfg.get("mode").and_then(Item::as_str).unwrap_or("split");
    let mut survey = Survey::default();
    let providers: BTreeSet<_> = tiers
        .into_iter()
        .flat_map(|t| t.iter())
        .filter_map(|(k, _)| k.split(':').next())
        .collect();
    for provider in providers {
        if mode != "split" && mode != format!("{provider}-only") {
            continue;
        }
        survey_provider(&mut survey, cfg, gate, provider, at);
    }
    survey
}

fn week_window(control: &Value) -> Option<&Value> {
    rows(&control["windows"])
        .iter()
        .find(|w| w["window"] == "week")
}

fn draining(control: &Value) -> bool {
    control["target_burn_pct_h"]
        .as_f64()
        .zip(week_window(control).and_then(|w| w["smoothed_pct_h"].as_f64()))
        .is_some_and(|(goal, burn)| goal - burn > crate::pace::limits::placement_drain_margin())
}

fn week_expiry(control: &Value) -> f64 {
    week_window(control)
        .and_then(|w| moment(&w["resets_at"]))
        .unwrap_or(f64::INFINITY)
}

pub(super) fn open_accounts(survey: &Survey, own: &str) -> Vec<String> {
    let mut open: Vec<_> = survey
        .reasons
        .iter()
        .filter(|(_, r)| r.is_empty())
        .map(|(n, _)| n.clone())
        .collect();
    let drains = |name: &str| draining(survey.controls[name]);
    let expiry = |name: &str| week_expiry(survey.controls[name]);
    let has_drain = open.iter().any(|n| drains(n));
    open.sort_by(|a, b| {
        if has_drain {
            (!drains(a))
                .cmp(&!drains(b))
                .then(expiry(a).total_cmp(&expiry(b)))
                .then((a != own).cmp(&(b != own)))
        } else {
            (a != own)
                .cmp(&(b != own))
                .then(expiry(a).total_cmp(&expiry(b)))
        }
        .then(a.cmp(b))
    });
    open
}

pub(super) fn unavailable(survey: &Survey, at: f64) -> Value {
    let reasons = &survey.reasons;
    if !reasons.is_empty() && reasons.values().all(|r| r == "floor") {
        return json!({"kind":"refused","reason":"every eligible account at its floor"});
    }
    queued(
        "accounts temporarily unavailable",
        minimum(&survey.etas, at),
    )
}
