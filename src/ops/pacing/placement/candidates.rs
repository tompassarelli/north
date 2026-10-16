use super::super::storage::{load, rows};
use super::super::{process, reservation, sampling, storage};
use super::{fresh, minimum, queued, release};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(in crate::ops::pacing) fn admit(request: &Value, at: f64) -> Result<Value, String> {
    let path = PathBuf::from(request["path"].as_str().ok_or("gate path required")?);
    let mut reasons = Vec::new();
    let mut holds = BTreeMap::new();
    let mut etas = Vec::new();
    for candidate in rows(&request["candidates"]) {
        let name = candidate["account"]
            .as_str()
            .ok_or("candidate account required")?;
        let provider = candidate["provider"]
            .as_str()
            .ok_or("candidate provider required")?;
        let gate = load(&path);
        let controls = &gate[provider]["controls"];
        let control = if controls[name].is_null() {
            &controls[provider]
        } else {
            &controls[name]
        };
        let reason = if fresh(control, at) {
            control["held_reason"].as_str().unwrap_or_default()
        } else {
            "stale"
        };
        holds.insert(name.to_owned(), reason.to_owned());
        if !reason.is_empty() {
            reasons.push(format!("{name} controller held: {reason}"));
            etas.extend(release(reason, control, at));
            continue;
        }
        let mut reserve = json!({"provider":provider,"account":name,"session":request["session"],"task":request["task"],
            "reservation":[storage::default("reservation_fallback_points"),storage::default("reservation_fallback_points")]});
        let account = if gate[provider]["accounts"][name].is_null() {
            &gate[provider]["accounts"][provider]
        } else {
            &gate[provider]["accounts"][name]
        };
        if account["used_pct"].is_number()
            && account["cap_pct"].is_number()
            && request.get("reservation").is_none()
        {
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
            if let Some(measured) = output
                .ok()
                .filter(|r| r.code == 0)
                .and_then(|r| serde_json::from_str::<Value>(&r.out).ok())
            {
                for (index, key) in ["expected", "existing"].iter().enumerate() {
                    if measured[*key].is_number() {
                        reserve["reservation"][index] = measured[*key].clone();
                    }
                }
            }
        }
        if let Some(calibration) = request.get("reservation") {
            reserve["reservation"] = calibration.clone();
        }
        if let Some(reason) = reservation::reserve(&path, &reserve, &process::fleet(), storage::now)? {
            reasons.push(reason);
            etas.push(at + 60.0);
        } else {
            return Ok(candidate.clone());
        }
    }
    for name in rows(&request["eligible"]).iter().filter_map(Value::as_str) {
        if holds.contains_key(name) {
            continue;
        }
        let gate = load(&path);
        let provider = name.split(':').next().unwrap_or_default();
        let controls = &gate[provider]["controls"];
        let control = if controls[name].is_null() {
            &controls[provider]
        } else {
            &controls[name]
        };
        let reason = if fresh(control, at) {
            if control.get("held_reason").is_none() {
                "stale"
            } else {
                control["held_reason"].as_str().unwrap_or_default()
            }
        } else {
            "stale"
        };
        holds.insert(name.to_owned(), reason.to_owned());
        etas.extend(release(reason, control, at));
    }
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
