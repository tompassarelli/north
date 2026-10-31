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
struct Terms<'a> {
    account: &'a str,
    until: &'a str,
    expiry: f64,
    maximum: u64,
    reason: &'a str,
}

fn terms<'a>(request: &'a Value, at: f64) -> Result<Terms<'a>, String> {
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
    Ok(Terms {
        account,
        until,
        expiry,
        maximum,
        reason,
    })
}

fn registered_lead<'a>(
    request: &Value,
    nodes: &'a [Value],
    account: &str,
) -> Result<&'a Value, String> {
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
    Ok(node)
}

fn grant_row(
    request: &Value,
    terms: &Terms,
    node: &Value,
    at: f64,
    operator: Option<&str>,
) -> Result<Value, String> {
    let until_utc = crate::foundation::clock::Timestamp::parse_rfc3339(terms.until)
        .ok_or("invalid UTC expiry")?
        .iso_micros_utc_offset()
        .replace(".000000", "");
    let mut row = json!({"account":terms.account,"lead":node["name"],"session":node["conversation"],"max_workers":terms.maximum,
        "until":terms.expiry,"until_utc":until_utc,"reason":terms.reason,"authority":"proxy","at":at,"granted_by":operator});
    for field in ["task", "uses"] {
        if !request[field].is_null() {
            row[field] = request[field].clone();
        }
    }
    Ok(row)
}

fn store_grant(directory: &Path, row: &Value, at: f64) -> Result<(), String> {
    let _lock = storage::lock(directory, "pace-control.lock", Some(6.0))?;
    let path = directory.join("pace-control.json");
    let mut state = load(&path);
    let mut grants: Vec<_> = rows(&state["grants"])
        .iter()
        .filter(|g| {
            number(&g["until"], 0.0) > at
                && !(g["account"] == row["account"] && g["lead"] == row["lead"])
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
    storage::append(&directory.join("pace-control.jsonl"), &audit)
}

pub(super) fn create(
    directory: &Path,
    request: &Value,
    nodes: &[Value],
    at: f64,
    operator: Option<&str>,
) -> Result<Value, String> {
    let terms = terms(request, at)?;
    let node = registered_lead(request, nodes, terms.account)?;
    let row = grant_row(request, &terms, node, at, operator)?;
    store_grant(directory, &row, at)?;
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
struct Holder<'a> {
    account: &'a str,
    node: &'a Value,
    request: &'a Value,
}

impl Holder<'_> {
    fn names(&self, row: &Value) -> bool {
        row["account"] == self.account
            && row["session"] == self.request["session"]
            && row["lead"] == self.node["name"]
    }
}

fn session_holder<'a>(
    request: &'a Value,
    nodes: &'a [Value],
    account: &'a str,
) -> Option<Holder<'a>> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    nodes
        .iter()
        .find(|n| {
            n["conversation"] == request["session"]
                && n["provider"] == provider
                && (provider != "claude" || n["account"] == label)
        })
        .map(|node| Holder {
            account,
            node,
            request,
        })
}

fn audit_rows(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .collect()
}

fn latest_grant<'a>(
    state: &'a Value,
    audit: &'a [Value],
    holder: &Holder,
) -> Option<(&'a Value, bool)> {
    let mut matching: Vec<_> = rows(&state["grants"])
        .iter()
        .map(|g| (g, true))
        .chain(
            audit
                .iter()
                .filter(|r| r["event"] == "proxy-grant")
                .map(|g| (g, false)),
        )
        .filter(|(g, _)| holder.names(g) && g["authority"] == "proxy")
        .collect();
    matching.sort_by(|(a, ar), (b, br)| {
        number(&a["at"], 0.0)
            .total_cmp(&number(&b["at"], 0.0))
            .then(ar.cmp(br))
    });
    matching.last().copied()
}

fn admissions_since(audit: &[Value], holder: &Holder, grant: &Value) -> f64 {
    audit
        .iter()
        .filter(|r| {
            r["event"] == "grant-admission"
                && holder.names(r)
                && number(&r["at"], 0.0) >= number(&grant["at"], 0.0)
        })
        .count() as f64
}

fn still_granted(
    audit: &[Value],
    holder: &Holder,
    (grant, retained): (&Value, bool),
    at: f64,
) -> bool {
    let consumed = admissions_since(audit, holder, grant);
    number(&grant["until"], 0.0) > at
        || (!grant["task"].is_null() && grant["task"] != holder.request["task"])
        || grant["uses"]
            .as_f64()
            .is_some_and(|uses| uses <= if retained { 0.0 } else { consumed })
}

fn holding_key(holder: &Holder, grant: &Value) -> String {
    json!([
        holder.account,
        holder.node["name"],
        holder.request["session"],
        grant["at"],
        grant["until"],
        grant["task"]
    ])
    .to_string()
}

fn already_holding(audit: &[Value], key: &str) -> bool {
    audit.iter().any(|r| {
        r["event"] == "grant-expired-holding"
            && r["key"]
                .as_str()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                == serde_json::from_str::<Value>(key).ok()
    })
}

fn announce_expiry(
    audit_path: &Path,
    holder: &Holder,
    grant: &Value,
    key: String,
    at: f64,
) -> Result<(), String> {
    let account = holder.account;
    let expiry =
        crate::foundation::clock::Timestamp::from_unix_seconds_f64(number(&grant["until"], 0.0))
            .iso_micros_utc_offset();
    let name = holder.node["name"].as_str().unwrap_or_default();
    let reason = holder.request["reason"].as_str().unwrap_or_default();
    let _ = process::send(
        name,
        &format!("GRANT-EXPIRED-HOLDING {name} {account} expired={expiry} {reason}"),
    );
    storage::append(
        audit_path,
        &json!({"event":"grant-expired-holding","at":at,"lead":name,"session":holder.request["session"],
        "account":account,"until":grant["until"],"reason":reason,"key":key}),
    )
}

pub(super) fn expired(
    directory: &Path,
    request: &Value,
    nodes: &[Value],
    at: f64,
) -> Result<(), String> {
    let account = request["account"].as_str().unwrap_or_default();
    let Some(holder) = session_holder(request, nodes, account) else {
        return Ok(());
    };
    let _lock = storage::lock(directory, "pace-control.lock", Some(6.0))?;
    let state = load(&directory.join("pace-control.json"));
    let audit_path = directory.join("pace-control.jsonl");
    let audit = audit_rows(&audit_path);
    let Some(latest) = latest_grant(&state, &audit, &holder) else {
        return Ok(());
    };
    if still_granted(&audit, &holder, latest, at) {
        return Ok(());
    }
    let key = holding_key(&holder, latest.0);
    if already_holding(&audit, &key) {
        return Ok(());
    }
    announce_expiry(&audit_path, &holder, latest.0, key, at)
}
