use super::{admission, control::Control, grants, storage, target};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use storage::{load, number, rows, save};

fn fallback<'a>(table: &'a Value, account: &str, provider: &str) -> &'a Value {
    if table[account].is_null() {
        &table[provider]
    } else {
        &table[account]
    }
}

fn cap_hold(
    row: &Value,
    control: &Control,
    account: &str,
    reservation: &[Value],
) -> Result<Option<String>, String> {
    let (Some(used), Some(cap)) = (row["used_pct"].as_f64(), row["cap_pct"].as_f64()) else {
        return Ok(None);
    };
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
    Ok(None)
}

fn account_hold(
    verdict: &Value,
    provider: &str,
    account: &str,
    reservation: &[Value],
    at: f64,
) -> Result<Option<String>, String> {
    let row = fallback(&verdict["accounts"], account, provider);
    let raw = fallback(&verdict["controls"], account, provider);
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
    cap_hold(row, &control, account, reservation)
}

fn awaiting_live(entry: &Value, at: f64) -> bool {
    (0.0..crate::pace::limits::reservation_ttl_seconds()).contains(&(at - number(&entry["at"], at)))
        && entry["counted_live"] != true
}

fn settle_pending(row: &mut Value, live: &BTreeSet<String>, at: f64) -> usize {
    let old: BTreeSet<_> = rows(&row["live"])
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut new_live = live.difference(&old).count();
    let pending = row["pending"].as_array_mut().unwrap();
    for entry in pending.iter_mut().filter(|r| awaiting_live(r, at)) {
        if new_live > 0 {
            entry["counted_live"] = json!(true);
            new_live -= 1;
        }
    }
    live.len() + pending.iter().filter(|r| awaiting_live(r, at)).count()
}

fn uses_exhausted(row: &Value, at: f64) -> bool {
    let held = rows(&row["pending"])
        .iter()
        .filter(|r| {
            r["confirmed"] != true
                && at - number(&r["at"], at) < crate::pace::limits::grant_hold_seconds()
        })
        .count();
    row["uses"].as_u64().is_some_and(|uses| held as u64 >= uses)
}

fn reserve_grant(
    state_path: &Path,
    state: &mut Value,
    grant: &Value,
    request: &Value,
    nodes: &[Value],
    at: f64,
) -> Result<Option<String>, String> {
    let account = request["account"].as_str().ok_or("account required")?;
    let live = grants::workers(grant, nodes);
    let row = state["grants"]
        .as_array_mut()
        .ok_or("invalid grants")?
        .iter_mut()
        .find(|g| g["account"] == account && g["lead"] == grant["lead"])
        .ok_or("grant disappeared")?;
    grants::normalize(row);
    let workers = settle_pending(row, &live, at);
    let maximum = number(&row["max_workers"], 0.0) as usize;
    let lead = grant["lead"].as_str().unwrap_or_default();
    if workers >= maximum {
        return Ok(Some(format!(
            "Hold: proxy grant for {lead} workers {workers}/{maximum}; measured count required."
        )));
    }
    if uses_exhausted(row, at) {
        return Ok(Some(format!(
            "Hold: proxy grant for {lead} has all remaining uses reserved."
        )));
    }
    row["live"] = json!(live);
    row["pending"]
        .as_array_mut()
        .unwrap()
        .push(json!({"at":at,"id":request["tool_use_id"]}));
    save(state_path, state)?;
    Ok(None)
}

fn reserve_pooled(
    path: &Path,
    gate: &mut Value,
    provider: &str,
    account: &str,
    at: f64,
) -> Result<Option<String>, String> {
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
    save(path, gate)?;
    Ok(None)
}

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
    if let Some(reservation) = request["reservation"].as_array()
        && let Some(hold) = account_hold(&gate[provider], provider, account, reservation, at)? {
            return Ok(Some(hold));
        }
    if let Some(grant) = grants::find(
        &state,
        account,
        &request["session"],
        &request["task"],
        at,
        nodes,
    ) {
        return reserve_grant(&state_path, &mut state, &grant, request, nodes, at);
    }
    reserve_pooled(path, &mut gate, provider, account, at)
}
