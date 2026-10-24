use super::survey::{Survey, open_accounts, survey_controls, unavailable};
use super::{field, place, private, queued, strings, tier_for};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use toml_edit::{DocumentMut, Item, TableLike};

pub(in crate::ops::pacing) struct Probes {
    pub configured: Value,
    pub observed: Value,
}

pub(in crate::ops::pacing) struct Routing<'a> {
    pub local: &'a str,
    pub defaults: &'a crate::router::policy::Defaults,
    pub account_order: &'a Value,
}

struct Prepared {
    cfg: DocumentMut,
    probe: Value,
}

struct Plan<'a> {
    text: &'a str,
    cfg: &'a DocumentMut,
    tiers: Option<&'a dyn TableLike>,
    provider: &'a str,
    requested: &'a str,
    floor: f64,
    difficulty: f64,
    defaults: &'a crate::router::policy::Defaults,
    local: &'a str,
    own: &'a str,
    at: f64,
}

#[derive(Default)]
struct Outcome {
    candidates: Vec<Value>,
    host_reasons: Vec<String>,
    laptop_queue: Option<Value>,
}

enum Landing {
    Launch,
    Laptop,
    Refused(String),
}

fn own_account(spawner: &Value, provider: &str) -> String {
    let own = spawner["account"].as_str().unwrap_or(provider);
    if !own.contains(':') && own != provider {
        format!("{provider}:{own}")
    } else {
        own.into()
    }
}

fn requested_tier<'a>(text: &str, spawner: &Value, tiers: Option<&'a dyn TableLike>) -> &'a str {
    let requested = if !field(text, "Tier").is_empty() {
        field(text, "Tier")
    } else {
        spawner["tier"]
            .as_str()
            .or_else(|| spawner["subagent_type"].as_str())
            .unwrap_or_default()
    };
    tiers
        .and_then(|t| {
            t.iter()
                .find(|(k, v)| {
                    *k == requested
                        || k.split_once(':').is_some_and(|(_, s)| s == requested)
                        || v.get("agent").and_then(Item::as_str) == Some(requested)
                })
                .map(|(k, _)| k)
        })
        .unwrap_or("claude:opus-medium")
}

fn tier_floor(tiers: Option<&dyn TableLike>, requested: &str) -> f64 {
    tiers
        .and_then(|t| t.get(requested))
        .and_then(|t| t.get("start"))
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .unwrap_or(0.0)
}

fn live_config(cfg: &DocumentMut, local: &str) -> DocumentMut {
    let mut cfg = cfg.clone();
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
    cfg
}

fn fixture_probe(cfg: &DocumentMut, configured: &Value) -> Value {
    let mut probe = json!({"laptop":"unavailable","credit":[false,"cloud credit: no snapshot"],"claude_rows":[],"claude_throttled":null});
    for (host, _) in cfg
        .get("hosts")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|h| h.iter())
    {
        probe[host] = json!([false, 0]);
    }
    for (key, value) in configured.as_object().into_iter().flatten() {
        probe[key] = value.clone();
    }
    probe
}

fn restrict_to_local(cfg: &mut DocumentMut, local: &str) {
    let order = strings(cfg.get("placement").and_then(|p| p.get("order")));
    let mut allowed = toml_edit::Array::new();
    for host in order {
        if host == local {
            allowed.push(host);
        }
    }
    cfg["placement"]["order"] = toml_edit::value(allowed);
}

fn prepare(text: &str, cfg: &DocumentMut, probes: Probes, local: &str) -> Prepared {
    let Probes {
        configured,
        observed,
    } = probes;
    let (mut cfg, mut probe) = if configured.is_null() {
        (live_config(cfg, local), json!({}))
    } else {
        (cfg.clone(), fixture_probe(cfg, &configured))
    };
    for (name, value) in observed.as_object().into_iter().flatten() {
        probe[name] = value.clone();
    }
    if private(text, &cfg) {
        restrict_to_local(&mut cfg, local);
    }
    Prepared { cfg, probe }
}

fn candidate_tier(plan: &Plan, provider: &str) -> Option<String> {
    let preferred =
        provider == plan.provider && plan.requested.split(':').next() == Some(plan.provider);
    if preferred {
        return Some(plan.requested.to_owned());
    }
    tier_for(
        plan.text,
        plan.cfg,
        provider,
        plan.floor,
        plan.difficulty,
        plan.defaults,
    )
}

/// Only Claude runs in Anthropic's cloud, and no other cloud lane remains a target.
fn host_config(cfg: &DocumentMut, provider: &str) -> DocumentMut {
    let mut host_cfg = cfg.clone();
    let mut filtered = toml_edit::Array::new();
    for h in strings(cfg.get("placement").and_then(|p| p.get("order"))) {
        let cloud = h.ends_with("-cloud");
        if !cloud || h == "anthropic-cloud" && provider == "claude" {
            filtered.push(h);
        }
    }
    host_cfg["placement"]["order"] = toml_edit::value(filtered);
    host_cfg
}

fn landing(host: &str, why: &str, local: &str) -> Landing {
    if host.starts_with("held:") {
        Landing::Refused(if why.is_empty() {
            host.into()
        } else {
            why.into()
        })
    } else if host != local && host != "laptop" {
        Landing::Refused(format!("unsupported launch host {host}: {why}"))
    } else if host == "laptop" {
        Landing::Laptop
    } else {
        Landing::Launch
    }
}

fn launch_candidate(plan: &Plan, name: &str, key: &str, host: &str) -> Option<Value> {
    let tier = plan.tiers.and_then(|t| t.get(key))?;
    let kind = if name == plan.own && host == plan.local {
        "native"
    } else {
        "launch"
    };
    Some(
        json!({"kind":kind,"provider":name.split(':').next().unwrap_or_default(),"account":name,"tier":key,
        "model":tier.get("model").and_then(Item::as_str),"effort":tier.get("effort").and_then(Item::as_str),"host":host}),
    )
}

fn consider_account(
    plan: &Plan,
    cfg: &DocumentMut,
    probe: &mut Value,
    name: &str,
    out: &mut Outcome,
) {
    let provider = name.split(':').next().unwrap_or_default();
    let Some(key) = candidate_tier(plan, provider) else {
        return;
    };
    let host_cfg = host_config(cfg, provider);
    let (host, why) = place(plan.text, &host_cfg, plan.at, probe);
    match landing(&host, &why, plan.local) {
        Landing::Refused(reason) => out.host_reasons.push(reason),
        Landing::Laptop => {
            let mut q = queued("laptop job: threads queue add laptop", plan.at + 60.0);
            q["host"] = json!("laptop");
            out.laptop_queue = Some(q);
        }
        Landing::Launch => out
            .candidates
            .extend(launch_candidate(plan, name, &key, &host)),
    }
}

fn provider_eligible(
    eligible: &mut Vec<String>,
    gate: &Value,
    survey: &Survey,
    account_order: &Value,
    provider: &str,
) {
    let controls = gate[provider]["controls"].as_object();
    if controls.is_none_or(|c| c.is_empty()) && survey.reasons.contains_key(provider) {
        eligible.push(provider.to_owned());
    }
    let ordered: Vec<String> = match account_order[provider].as_array() {
        Some(names) => names
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        None => controls
            .into_iter()
            .flat_map(|c| c.keys().cloned())
            .collect(),
    };
    eligible.extend(
        ordered
            .into_iter()
            .filter(|name| survey.reasons.contains_key(name)),
    );
}

fn eligible_accounts(
    tiers: Option<&dyn TableLike>,
    gate: &Value,
    survey: &Survey,
    account_order: &Value,
) -> Vec<String> {
    let mut eligible = Vec::new();
    let mut seen = BTreeSet::new();
    for (tier, _) in tiers.into_iter().flat_map(|t| t.iter()) {
        let provider = tier.split(':').next().unwrap_or_default();
        if seen.insert(provider) {
            provider_eligible(&mut eligible, gate, survey, account_order, provider);
        }
    }
    eligible
}

fn no_candidate(out: Outcome, at: f64) -> Value {
    out.laptop_queue.unwrap_or_else(|| {
        let reason = if out.host_reasons.is_empty() {
            "no open tier admits the requested band".to_owned()
        } else {
            out.host_reasons.join("; ")
        };
        queued(&reason, at + 60.0)
    })
}

pub(in crate::ops::pacing) fn spawn(
    text: &str,
    spawner: &Value,
    gate: &Value,
    cfg: &DocumentMut,
    at: f64,
    probes: Probes,
    routing: Routing<'_>,
) -> Value {
    let Routing { local, defaults, account_order } = routing;
    let provider = spawner["provider"].as_str().unwrap_or_default();
    let own = own_account(spawner, provider);
    let tiers = cfg.get("tiers").and_then(Item::as_table_like);
    let requested = requested_tier(text, spawner, tiers);
    let floor = tier_floor(tiers, requested);
    let difficulty = field(text, "Difficulty")
        .parse::<f64>()
        .unwrap_or(floor)
        .max(floor);
    let survey = survey_controls(cfg, gate, tiers, at);
    let open = open_accounts(&survey, &own);
    if open.is_empty() {
        return unavailable(&survey, at);
    }
    let Prepared { cfg, mut probe } = prepare(text, cfg, probes, local);
    let plan = Plan {
        text,
        cfg: &cfg,
        tiers,
        provider,
        requested,
        floor,
        difficulty,
        defaults,
        local,
        own: &own,
        at,
    };
    let mut out = Outcome::default();
    for name in &open {
        consider_account(&plan, &cfg, &mut probe, name, &mut out);
    }
    let Some(first) = out.candidates.first() else {
        return no_candidate(out, at);
    };
    let mut result = first.clone();
    result["candidates"] = json!(out.candidates);
    result["eligible"] = json!(eligible_accounts(tiers, gate, &survey, account_order));
    result
}
