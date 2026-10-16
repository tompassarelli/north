use super::{admission, control::Control, grants, storage, target};
use serde_json::{Value, json};
use std::path::Path;
use storage::{load, number, rows, save};

pub(super) fn reserve(
    path: &Path,
    request: &Value,
    nodes: &[Value],
    clock: impl Fn() -> f64,
) -> Result<Option<String>, String> {
    let directory = path.parent().ok_or("gate has no directory")?;
    let provider = request["provider"].as_str().ok_or("provider required")?;
    let account = request["account"].as_str().ok_or("account required")?;
    let wait = number(&request["wait"], 6.0);
    let _lock = match storage::lock(directory, "pace-control.lock", Some(wait)) {
        Ok(lock) => lock,
        Err(_) => {
            return Ok(Some(format!(
                "Hold: {account} pace controller stayed locked for {wait:.0}s; retry after its fresh reading."
            )));
        }
    };
    let at = clock();
    let mut gate = load(path);
    let state_path = directory.join("pace-control.json");
    let mut state = load(&state_path);
    if let Some(reservation) = request["reservation"].as_array() {
        let verdict = &gate[provider];
        let row = if verdict["accounts"][account].is_null() {
            &verdict["accounts"][provider]
        } else {
            &verdict["accounts"][account]
        };
        let raw = if verdict["controls"][account].is_null() {
            &verdict["controls"][provider]
        } else {
            &verdict["controls"][account]
        };
        let control: Control = serde_json::from_value(raw.clone()).unwrap_or_default();
        if !admission::is_fresh(&control, at) {
            return Ok(Some(format!(
                "Hold: {account} pace controller reading is not fresh."
            )));
        }
        if raw["held_reason"].as_str().is_some() || raw.get("held_reason").is_none() {
            return Ok(Some(format!(
                "Hold: {account} controller held: {}.",
                raw["held_reason"].as_str().unwrap_or("stale")
            )));
        }
        if let (Some(used), Some(cap)) = (row["used_pct"].as_f64(), row["cap_pct"].as_f64()) {
            let Some(workers) = control.workers else {
                return Ok(Some(format!(
                    "Hold: {account} live worker attribution unavailable."
                )));
            };
            let expected = reservation
                .first()
                .and_then(Value::as_f64)
                .ok_or("expected reservation required")?;
            let existing = reservation
                .get(1)
                .and_then(Value::as_f64)
                .ok_or("existing reservation required")?;
            let projected = used + expected + existing * workers as f64;
            if used >= cap || projected - cap > 1e-9 {
                return Ok(Some(format!(
                    "Hold: {account} projected account reservation exceeds cap {cap}%."
                )));
            }
        }
    }
    if let Some(grant) = grants::find(
        &state,
        account,
        &request["session"],
        &request["task"],
        at,
        nodes,
    ) {
        let live = grants::workers(&grant, nodes);
        let row = state["grants"]
            .as_array_mut()
            .ok_or("invalid grants")?
            .iter_mut()
            .find(|g| g["account"] == account && g["lead"] == grant["lead"])
            .ok_or("grant disappeared")?;
        grants::normalize(row);
        let old: std::collections::BTreeSet<_> = rows(&row["live"])
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut new_live = live.difference(&old).count();
        let pending = row["pending"].as_array_mut().unwrap();
        for entry in pending.iter_mut().filter(|r| {
            (0.0..crate::pace::limits::reservation_ttl_seconds())
                .contains(&(at - number(&r["at"], at)))
                && r["counted_live"] != true
        }) {
            if new_live > 0 {
                entry["counted_live"] = json!(true);
                new_live -= 1;
            }
        }
        let workers = live.len()
            + pending
                .iter()
                .filter(|r| {
                    (0.0..crate::pace::limits::reservation_ttl_seconds())
                        .contains(&(at - number(&r["at"], at)))
                        && r["counted_live"] != true
                })
                .count();
        let maximum = number(&row["max_workers"], 0.0) as usize;
        let lead = grant["lead"].as_str().unwrap_or_default();
        if workers >= maximum {
            return Ok(Some(format!(
                "Hold: proxy grant for {lead} workers {workers}/{maximum}; measured count required."
            )));
        }
        let held = rows(&row["pending"])
            .iter()
            .filter(|r| {
                r["confirmed"] != true
                    && at - number(&r["at"], at) < crate::pace::limits::grant_hold_seconds()
            })
            .count();
        if row["uses"].as_u64().is_some_and(|uses| held as u64 >= uses) {
            return Ok(Some(format!(
                "Hold: proxy grant for {lead} has all remaining uses reserved."
            )));
        }
        row["live"] = json!(live);
        row["pending"]
            .as_array_mut()
            .unwrap()
            .push(json!({"at":at,"id":request["tool_use_id"]}));
        save(&state_path, &state)?;
        return Ok(None);
    }
    let controls = &mut gate[provider]["controls"];
    let key = if controls[account].is_null() {
        provider
    } else {
        account
    };
    if controls[key].is_null() {
        return Ok(Some(format!(
            "Hold: {account} pace controller has no account reading."
        )));
    }
    let control: Control =
        serde_json::from_value(controls[key].clone()).map_err(|e| e.to_string())?;
    if let Some(reason) = admission::refusal(account, &control, at, target::target_for(account, at))
    {
        return Ok(Some(reason));
    }
    controls[key]["workers"] = json!(control.workers.unwrap_or(0) + 1);
    save(path, &gate)?;
    Ok(None)
}
