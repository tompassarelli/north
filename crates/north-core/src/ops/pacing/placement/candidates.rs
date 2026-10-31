use super::super::storage::{load, rows};
use super::super::{process, reservation, sampling, storage};
use super::{fresh, minimum, queued, release};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Refusals {
    reasons: Vec<String>,
    holds: BTreeMap<String, String>,
    etas: Vec<f64>,
}

fn control_of<'a>(gate: &'a Value, provider: &str, name: &str) -> &'a Value {
    let controls = &gate[provider]["controls"];
    if controls[name].is_null() {
        &controls[provider]
    } else {
        &controls[name]
    }
}

fn candidate_hold(control: &Value, at: f64) -> &str {
    if fresh(control, at) {
        control["held_reason"].as_str().unwrap_or_default()
    } else {
        "stale"
    }
}

fn eligible_hold(control: &Value, at: f64) -> &str {
    if fresh(control, at) && control.get("held_reason").is_some() {
        control["held_reason"].as_str().unwrap_or_default()
    } else {
        "stale"
    }
}

fn measured_reservation(candidate: &Value, provider: &str, name: &str) -> Option<Value> {
    let measurement = json!({"provider":provider,"model":candidate["model"],"effort":candidate["effort"],"label":name.split_once(':').map(|(_,label)|label).unwrap_or_default()});
    let output = process::run(
        "python3",
        &[
            "-I",
            &sampling::root()
                .join("harness/lib/usage.py")
                .to_string_lossy(),
            "--reservation-json",
        ],
        Some(&measurement.to_string()),
        30.0,
    );
    output
        .ok()
        .filter(|r| r.code == 0)
        .and_then(|r| serde_json::from_str::<Value>(&r.out).ok())
}

fn reservation_request(
    request: &Value,
    candidate: &Value,
    gate: &Value,
    provider: &str,
    name: &str,
) -> Value {
    let fallback = storage::default("reservation_fallback_points");
    let mut reserve = json!({"provider":provider,"account":name,"session":request["session"],"task":request["task"],
        "reservation":[fallback,fallback]});
    let account = if gate[provider]["accounts"][name].is_null() {
        &gate[provider]["accounts"][provider]
    } else {
        &gate[provider]["accounts"][name]
    };
    let measurable = account["used_pct"].is_number()
        && account["cap_pct"].is_number()
        && request.get("reservation").is_none();
    if measurable
        && let Some(measured) = measured_reservation(candidate, provider, name) {
            for (index, key) in ["expected", "existing"].iter().enumerate() {
                if measured[*key].is_number() {
                    reserve["reservation"][index] = measured[*key].clone();
                }
            }
        }
    if let Some(calibration) = request.get("reservation") {
        reserve["reservation"] = calibration.clone();
    }
    reserve
}

fn try_candidate(
    request: &Value,
    path: &Path,
    at: f64,
    candidate: &Value,
    refusals: &mut Refusals,
) -> Result<bool, String> {
    let name = candidate["account"]
        .as_str()
        .ok_or("candidate account required")?;
    let provider = candidate["provider"]
        .as_str()
        .ok_or("candidate provider required")?;
    let gate = load(path);
    let control = control_of(&gate, provider, name);
    let reason = candidate_hold(control, at);
    refusals.holds.insert(name.to_owned(), reason.to_owned());
    if !reason.is_empty() {
        refusals
            .reasons
            .push(format!("{name} controller held: {reason}"));
        refusals.etas.extend(release(reason, control, at));
        return Ok(false);
    }
    let reserve = reservation_request(request, candidate, &gate, provider, name);
    match reservation::reserve(path, &reserve, &process::fleet(), storage::now)? {
        Some(reason) => {
            refusals.reasons.push(reason);
            refusals.etas.push(at + 60.0);
            Ok(false)
        }
        None => Ok(true),
    }
}

fn hold_eligible(request: &Value, path: &Path, at: f64, refusals: &mut Refusals) {
    for name in rows(&request["eligible"]).iter().filter_map(Value::as_str) {
        if refusals.holds.contains_key(name) {
            continue;
        }
        let gate = load(path);
        let provider = name.split(':').next().unwrap_or_default();
        let control = control_of(&gate, provider, name);
        let reason = eligible_hold(control, at);
        refusals.holds.insert(name.to_owned(), reason.to_owned());
        refusals.etas.extend(release(reason, control, at));
    }
}

pub(in crate::ops::pacing) fn admit(request: &Value, at: f64) -> Result<Value, String> {
    let path = PathBuf::from(request["path"].as_str().ok_or("gate path required")?);
    let mut refusals = Refusals::default();
    for candidate in rows(&request["candidates"]) {
        if try_candidate(request, &path, at, candidate, &mut refusals)? {
            return Ok(candidate.clone());
        }
    }
    hold_eligible(request, &path, at, &mut refusals);
    let Refusals {
        reasons,
        holds,
        etas,
    } = refusals;
    if !holds.is_empty() && holds.values().all(|r| r == "floor") {
        return Ok(json!({"kind":"refused","reason":"every eligible account at its floor"}));
    }
    let reason = if reasons.is_empty() {
        "accounts temporarily unavailable".to_owned()
    } else {
        reasons.join("; ")
    };
    Ok(queued(&reason, minimum(&etas, at)))
}
