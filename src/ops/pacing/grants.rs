use super::{process, storage};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use storage::{load, number, rows, save};

pub(super) fn find(
    state: &Value,
    account: &str,
    session: &Value,
    task: &Value,
    at: f64,
    nodes: &[Value],
) -> Option<Value> {
    rows(&state["grants"])
        .iter()
        .find(|g| {
            g["account"] == account
                && g["session"] == *session
                && at < number(&g["until"], 0.0)
                && (g["task"].is_null() || g["task"] == *task)
                && number(&g["uses"], 1.0) > 0.0
                && nodes
                    .iter()
                    .any(|n| n["name"] == g["lead"] && n["conversation"] == *session)
        })
        .cloned()
}
pub(super) fn workers(grant: &Value, nodes: &[Value]) -> BTreeSet<String> {
    let account = grant["account"].as_str().unwrap_or_default();
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let mut ids = BTreeSet::from([grant["lead"].as_str().unwrap_or_default().to_owned()]);
    loop {
        let children: Vec<_> = nodes
            .iter()
            .filter(|n| n["parent"].as_str().is_some_and(|p| ids.contains(p)))
            .filter_map(|n| n["name"].as_str().map(str::to_owned))
            .collect();
        let before = ids.len();
        ids.extend(children);
        if ids.len() == before {
            break;
        }
    }
    nodes
        .iter()
        .filter(|n| {
            n["name"] != grant["lead"]
                && n["name"].as_str().is_some_and(|s| ids.contains(s))
                && n["provider"] == provider
                && n["status"] == "working"
                && (provider != "claude" || n["account"] == label)
        })
        .filter_map(|n| {
            n["conversation"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .collect()
}
pub(super) fn create(
    directory: &Path,
    request: &Value,
    nodes: &[Value],
    at: f64,
    operator: Option<&str>,
) -> Result<Value, String> {
    let account = request["account"].as_str().ok_or("account required")?;
    let until = request["until"].as_str().ok_or("--until required")?;
    let expiry = super::target::seconds(until).ok_or("--until must be a future UTC timestamp")?;
    if expiry <= at
        || !(until.ends_with('Z') || until.ends_with("+00:00") || until.ends_with("-00:00"))
    {
        return Err("--until must be a future UTC timestamp".into());
    }
    let maximum = request["max_workers"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or("positive worker limit required")?;
    let reason = request["reason"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("reason required")?;
    if !request["uses"].is_null()
        && (request["uses"].as_u64().is_none_or(|n| n == 0)
            || request["task"].as_str().is_none_or(str::is_empty))
    {
        return Err("positive uses and task required".into());
    }
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let node = nodes
        .iter()
        .find(|n| {
            n["name"] == request["lead"]
                && n["provider"] == provider
                && n["conversation"].as_str().is_some_and(|s| !s.is_empty())
        })
        .ok_or("--for must name an active registered lead on that provider")?;
    if provider == "claude" && node["account"] != label {
        return Err("grant account differs from the lead's billed account".into());
    }
    let until_utc = crate::foundation::clock::Timestamp::parse_rfc3339(until)
        .ok_or("invalid UTC expiry")?
        .iso_micros_utc_offset()
        .replace(".000000", "");
    let mut row = json!({"account":account,"lead":node["name"],"session":node["conversation"],"max_workers":maximum,
        "until":expiry,"until_utc":until_utc,"reason":reason,"authority":"proxy","at":at,"granted_by":operator});
    for field in ["task", "uses"] {
        if !request[field].is_null() {
            row[field] = request[field].clone();
        }
    }
    let _lock = storage::lock(directory, "pace-control.lock", Some(6.0))?;
    let path = directory.join("pace-control.json");
    let mut state = load(&path);
    let mut grants: Vec<_> = rows(&state["grants"])
        .iter()
        .filter(|g| {
            number(&g["until"], 0.0) > at && !(g["account"] == account && g["lead"] == node["name"])
        })
        .cloned()
        .collect();
    grants.push(row.clone());
    state["grants"] = json!(grants);
    for key in ["accounts", "providers"] {
        if state[key].is_null() {
            state[key] = json!({});
        }
    }
    save(&path, &state)?;
    let mut audit = row.clone();
    audit["event"] = json!("proxy-grant");
    storage::append(&directory.join("pace-control.jsonl"), &audit)?;
    Ok(row)
}
pub(super) fn normalize(row: &mut Value) {
    let pending: Vec<_> = rows(&row["pending"])
        .iter()
        .map(|r| {
            if r.is_object() {
                r.clone()
            } else {
                json!({"at":r,"confirmed":true})
            }
        })
        .collect();
    row["pending"] = json!(pending);
}
pub(super) fn confirm(
    directory: &Path,
    session: &Value,
    id: &Value,
    launched: bool,
    clock: impl Fn() -> f64,
) -> Result<(), String> {
    if id.as_str().is_none_or(str::is_empty) || !directory.join("pace-control.json").exists() {
        return Ok(());
    }
    let _lock = storage::lock(directory, "pace-control.lock", Some(6.0))?;
    let path = directory.join("pace-control.json");
    let mut state = load(&path);
    let Some(grants) = state["grants"].as_array_mut() else {
        return Ok(());
    };
    for row in grants {
        if row["session"] != *session {
            continue;
        }
        normalize(row);
        let Some(index) = rows(&row["pending"])
            .iter()
            .position(|r| r["id"] == *id && r["confirmed"] != true)
        else {
            continue;
        };
        if !launched {
            row["pending"].as_array_mut().unwrap().remove(index);
            return save(&path, &state);
        }
        let reserved = row["pending"][index]["at"].clone();
        row["pending"][index]["confirmed"] = json!(true);
        if let Some(uses) = row["uses"].as_i64() {
            row["uses"] = json!(uses - 1);
        }
        let at = clock();
        let audit = json!({"event":"grant-admission","at":at,"account":row["account"],"session":session,
            "lead":row["lead"],"tool_use_id":id,"reserved_at":reserved});
        if at - number(&reserved, at) >= crate::pace::limits::grant_hold_seconds() {
            let mut late = audit.clone();
            late["event"] = json!("grant-late-confirm");
            late["uses"] = row["uses"].clone();
            storage::append(&directory.join("pace-control.jsonl"), &late)?;
        }
        save(&path, &state)?;
        storage::append(&directory.join("pace-control.jsonl"), &audit)?;
        return Ok(());
    }
    Ok(())
}
pub(super) fn expired(
    directory: &Path,
    request: &Value,
    nodes: &[Value],
    at: f64,
) -> Result<(), String> {
    let account = request["account"].as_str().unwrap_or_default();
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let Some(node) = nodes.iter().find(|n| {
        n["conversation"] == request["session"]
            && n["provider"] == provider
            && (provider != "claude" || n["account"] == label)
    }) else {
        return Ok(());
    };
    let _lock = storage::lock(directory, "pace-control.lock", Some(6.0))?;
    let state = load(&directory.join("pace-control.json"));
    let audit_path = directory.join("pace-control.jsonl");
    let audit: Vec<Value> = std::fs::read_to_string(&audit_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .collect();
    let mut matching: Vec<_> = rows(&state["grants"])
        .iter()
        .map(|g| (g, true))
        .chain(
            audit
                .iter()
                .filter(|r| r["event"] == "proxy-grant")
                .map(|g| (g, false)),
        )
        .filter(|(g, _)| {
            g["account"] == account
                && g["session"] == request["session"]
                && g["lead"] == node["name"]
                && g["authority"] == "proxy"
        })
        .collect();
    matching.sort_by(|(a, ar), (b, br)| {
        number(&a["at"], 0.0)
            .total_cmp(&number(&b["at"], 0.0))
            .then(ar.cmp(br))
    });
    let Some((grant, retained)) = matching.last() else {
        return Ok(());
    };
    let consumed = audit
        .iter()
        .filter(|r| {
            r["event"] == "grant-admission"
                && r["account"] == account
                && r["session"] == request["session"]
                && r["lead"] == node["name"]
                && number(&r["at"], 0.0) >= number(&grant["at"], 0.0)
        })
        .count() as f64;
    if number(&grant["until"], 0.0) > at
        || (!grant["task"].is_null() && grant["task"] != request["task"])
        || grant["uses"]
            .as_f64()
            .is_some_and(|uses| uses <= if *retained { 0.0 } else { consumed })
    {
        return Ok(());
    }
    let key = json!([
        account,
        node["name"],
        request["session"],
        grant["at"],
        grant["until"],
        grant["task"]
    ])
    .to_string();
    if audit.iter().any(|r| {
        r["event"] == "grant-expired-holding"
            && r["key"]
                .as_str()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                == serde_json::from_str::<Value>(&key).ok()
    }) {
        return Ok(());
    }
    let expiry =
        crate::foundation::clock::Timestamp::from_unix_seconds_f64(number(&grant["until"], 0.0))
            .iso_micros_utc_offset();
    let name = node["name"].as_str().unwrap_or_default();
    let reason = request["reason"].as_str().unwrap_or_default();
    let _ = process::send(
        name,
        &format!("GRANT-EXPIRED-HOLDING {name} {account} expired={expiry} {reason}"),
    );
    storage::append(
        &audit_path,
        &json!({"event":"grant-expired-holding","at":at,"lead":name,"session":request["session"],
        "account":account,"until":grant["until"],"reason":reason,"key":key}),
    )
}
