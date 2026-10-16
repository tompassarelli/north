use super::super::storage::rows;
use super::{
    facets, field, fresh, hydrate, minimum, moment, overflow, place, private, queued, release,
    strings, tier_for,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item};

pub(in crate::ops::pacing) struct Probes {
    pub configured: Value,
    pub observed: Value,
}

pub(in crate::ops::pacing) fn spawn(
    text: &str,
    spawner: &Value,
    gate: &Value,
    cfg: &DocumentMut,
    at: f64,
    probes: Probes,
    local: &str,
    defaults: &crate::router::policy::Defaults,
    account_order: &Value,
) -> Value {
    let provider = spawner["provider"].as_str().unwrap_or_default();
    let own = spawner["account"].as_str().unwrap_or(provider);
    let own = if !own.contains(':') && own != provider {
        format!("{provider}:{own}")
    } else {
        own.into()
    };
    let tiers = cfg.get("tiers").and_then(Item::as_table_like);
    let requested = if !field(text, "Tier").is_empty() {
        field(text, "Tier")
    } else {
        spawner["tier"]
            .as_str()
            .or_else(|| spawner["subagent_type"].as_str())
            .unwrap_or_default()
    };
    let requested = tiers
        .and_then(|t| {
            t.iter()
                .find(|(k, v)| {
                    *k == requested
                        || k.split_once(':').is_some_and(|(_, s)| s == requested)
                        || v.get("agent").and_then(Item::as_str) == Some(requested)
                })
                .map(|(k, _)| k)
        })
        .unwrap_or("claude:opus-medium");
    let floor = tiers
        .and_then(|t| t.get(requested))
        .and_then(|t| t.get("start"))
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .unwrap_or(0.0);
    let difficulty = field(text, "Difficulty")
        .parse::<f64>()
        .unwrap_or(floor)
        .max(floor);
    let mode = cfg.get("mode").and_then(Item::as_str).unwrap_or("split");
    let mut reasons = BTreeMap::new();
    let mut controls = BTreeMap::new();
    let mut etas = Vec::new();
    let providers: BTreeSet<_> = tiers
        .into_iter()
        .flat_map(|t| t.iter())
        .filter_map(|(k, _)| k.split(':').next())
        .collect();
    for p in providers {
        if mode != "split" && mode != format!("{p}-only") {
            continue;
        }
        let policy = cfg.get("accounts").and_then(|a| a.get(p));
        if gate[p]["controls"].as_object().is_none_or(|c| c.is_empty())
            && policy
                .and_then(|s| s.get("auto_route"))
                .and_then(Item::as_bool)
                != Some(false)
        {
            reasons.insert(p.to_owned(), "stale".to_owned());
            etas.push(at + 60.0);
        }
        for (name, control) in gate[p]["controls"].as_object().into_iter().flatten() {
            if control["auto_route"] == false
                || control["auto_route"].is_null()
                    && policy
                        .and_then(|s| s.get("auto_route"))
                        .and_then(Item::as_bool)
                        == Some(false)
            {
                continue;
            }
            let billing = cfg
                .get("billing")
                .and_then(|b| b.get(name).or_else(|| b.get(p)))
                .and_then(|b| b.get("standard"))
                .and_then(Item::as_str)
                .unwrap_or("plan");
            let reason = if billing != "plan" {
                "blocked"
            } else if !fresh(control, at) || control.get("held_reason").is_none() {
                "stale"
            } else {
                control["held_reason"].as_str().unwrap_or_default()
            };
            etas.extend(release(reason, control, at));
            reasons.insert(name.clone(), reason.to_owned());
            controls.insert(name.clone(), control);
        }
    }
    let mut open: Vec<_> = reasons
        .iter()
        .filter(|(_, r)| r.is_empty())
        .map(|(n, _)| n.clone())
        .collect();
    if open.is_empty() {
        if !reasons.is_empty() && reasons.values().all(|r| r == "floor") {
            return json!({"kind":"refused","reason":"every eligible account at its floor"});
        }
        return queued("accounts temporarily unavailable", minimum(&etas, at));
    }
    let draining = |name: &str| {
        let c = controls[name];
        let week = rows(&c["windows"]).iter().find(|w| w["window"] == "week");
        c["target_burn_pct_h"]
            .as_f64()
            .zip(week.and_then(|w| w["smoothed_pct_h"].as_f64()))
            .is_some_and(|(goal, burn)| goal - burn > crate::pace::limits::placement_drain_margin())
    };
    let expiry = |name: &str| {
        rows(&controls[name]["windows"])
            .iter()
            .find(|w| w["window"] == "week")
            .and_then(|w| moment(&w["resets_at"]))
            .unwrap_or(f64::INFINITY)
    };
    let has_drain = open.iter().any(|n| draining(n));
    open.sort_by(|a, b| {
        if has_drain {
            (!draining(a))
                .cmp(&!draining(b))
                .then(expiry(a).total_cmp(&expiry(b)))
                .then((a != &own).cmp(&(b != &own)))
        } else {
            (a != &own)
                .cmp(&(b != &own))
                .then(expiry(a).total_cmp(&expiry(b)))
        }
        .then(a.cmp(b))
    });
    let Probes {
        configured: mut probe,
        observed,
    } = probes;
    let fixture = !probe.is_null();
    let mut cfg = cfg.clone();
    if probe.is_null() {
        probe = json!({});
        let original = cfg
            .get("hosts")
            .and_then(|h| h.get(local))
            .and_then(|h| h.get("resident_cap"))
            .and_then(Item::as_integer)
            .unwrap_or(i64::MAX);
        cfg["hosts"][local]["resident_cap"] = toml_edit::value(original);
        let mut order = toml_edit::Array::new();
        order.push(local);
        order.push("laptop");
        cfg["placement"]["order"] = toml_edit::value(order);
    }
    if fixture {
        let mut defaults = json!({"laptop":"unavailable","credit":[false,"cloud credit: no snapshot"],"claude_rows":[],"claude_throttled":null});
        for (host, _) in cfg
            .get("hosts")
            .and_then(Item::as_table_like)
            .into_iter()
            .flat_map(|h| h.iter())
        {
            defaults[host] = json!([false, 0]);
        }
        for (key, value) in probe.as_object().into_iter().flatten() {
            defaults[key] = value.clone();
        }
        probe = defaults;
    }
    for (name, value) in observed.as_object().into_iter().flatten() {
        probe[name] = value.clone();
    }
    if private(text, &cfg) {
        let order = strings(cfg.get("placement").and_then(|p| p.get("order")));
        let mut allowed = toml_edit::Array::new();
        for host in order {
            if host == local {
                allowed.push(host);
            }
        }
        cfg["placement"]["order"] = toml_edit::value(allowed);
    }
    if probe["claude_paces"].is_null() {
        probe["claude_paces"] = gate["claude"]["paces"].clone();
    }
    let code_only = !facets(text).1 && !private(text, &cfg);
    if code_only {
        hydrate(&mut probe);
    }
    let overflowed = code_only && overflow(&cfg, &probe, at).is_some();
    let mut candidates = Vec::new();
    let mut host_reasons = Vec::new();
    let mut laptop_queue = None;
    for name in open {
        let p = name.split(':').next().unwrap_or_default();
        let key = if p == provider && requested.split(':').next() == Some(provider) {
            Some(requested.to_owned())
        } else {
            tier_for(text, &cfg, p, floor, difficulty, defaults)
        }
        .or_else(|| {
            (overflowed && p == "codex" && tiers.is_some_and(|t| t.get("codex:sol-high").is_some()))
                .then(|| "codex:sol-high".into())
        });
        let Some(key) = key else {
            continue;
        };
        let mut host_cfg = cfg.clone();
        let order = strings(cfg.get("placement").and_then(|p| p.get("order")));
        let mut filtered = toml_edit::Array::new();
        let cloud = if p == "claude" {
            "anthropic-cloud"
        } else {
            "codex-cloud"
        };
        for h in order {
            if !["anthropic-cloud", "codex-cloud"].contains(&h.as_str()) || h == cloud {
                filtered.push(h);
            }
        }
        host_cfg["placement"]["order"] = toml_edit::value(filtered);
        let (host, why) = place(text, &host_cfg, at, &mut probe, name == "claude:a");
        if host == "codex" && name == "claude:a" {
            host_reasons.push(format!("claude:a overflow: {why}"));
            continue;
        }
        if host.starts_with("held:") {
            host_reasons.push(if why.is_empty() { host } else { why });
            continue;
        }
        if host != local && host != "laptop" {
            host_reasons.push(format!("unsupported launch host {host}: {why}"));
            continue;
        }
        if host == "laptop" {
            let mut q = queued("laptop job: threads queue add laptop", at + 60.0);
            q["host"] = json!("laptop");
            laptop_queue = Some(q);
            continue;
        }
        if let Some(tier) = tiers.and_then(|t| t.get(&key)) {
            candidates.push(json!({"kind":if name==own && host==local{"native"}else{"launch"},"provider":p,"account":name,"tier":key,
                "model":tier.get("model").and_then(Item::as_str),"effort":tier.get("effort").and_then(Item::as_str),"host":host}));
        }
    }
    if let Some(first) = candidates.first() {
        let mut result = first.clone();
        result["candidates"] = json!(candidates);
        let mut eligible = Vec::new();
        let mut providers = BTreeSet::new();
        for (tier, _) in tiers.into_iter().flat_map(|t| t.iter()) {
            let provider = tier.split(':').next().unwrap_or_default();
            if !providers.insert(provider) {
                continue;
            }
            if gate[provider]["controls"]
                .as_object()
                .is_none_or(|c| c.is_empty())
                && reasons.contains_key(provider)
            {
                eligible.push(provider.to_owned());
            }
            let ordered = account_order[provider]
                .as_array()
                .cloned()
                .unwrap_or_else(|| {
                    gate[provider]["controls"]
                        .as_object()
                        .into_iter()
                        .flat_map(|c| c.keys().map(|s| json!(s)))
                        .collect()
                });
            eligible.extend(
                ordered
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|name| reasons.contains_key(*name))
                    .map(str::to_owned),
            );
        }
        result["eligible"] = json!(eligible);
        return result;
    }
    laptop_queue.unwrap_or_else(|| {
        let reason = if host_reasons.is_empty() {
            "no open tier admits the requested band".to_owned()
        } else {
            host_reasons.join("; ")
        };
        queued(&reason, at + 60.0)
    })
}
