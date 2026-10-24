use super::{admission, control::Control, sampling, storage, target};
use crate::error::NorthResult;
use crate::fleet::roster::Roster;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use storage::{number, rows};
use toml_edit::{DocumentMut, Item};

mod candidates;
pub(crate) mod gate;
mod hosts;
mod launch;
mod survey;
mod tiers;
use candidates::admit;
use hosts::{credit, laptop, resident, signed_in};
pub(super) use launch::{Probes, Routing, spawn};
pub(super) use tiers::routing_defaults;
use tiers::tier_for;

pub(super) fn field<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|line| {
            line.trim()
                .split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.trim())
        })
        .unwrap_or_default()
}
fn facets(text: &str) -> (String, bool, BTreeSet<String>) {
    let needs: BTreeSet<_> = text
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("Needs"))
        .map(|(_, v)| v.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
        .split(|c: char| c.is_whitespace() || c == ',' || c == '|')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let native = field(text, "Verify").split_whitespace().next() == Some("native")
        || needs.contains("native")
        || needs.contains("sign-ins");
    (field(text, "Item").into(), native, needs)
}
fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn serving_host(needs: &str) -> Option<String> {
    gate::serving(&sampling::config().ok()?, needs, &local())
}
fn local() -> String {
    crate::platform::host::name()
}
fn operator() -> PathBuf {
    std::env::var_os("AGENTS_OPERATOR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            sampling::policy_path()
                .parent()
                .unwrap_or(Path::new("."))
                .to_owned()
        })
}
fn stamp(at: f64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds_f64(at)
        .iso_micros_utc_offset()
        .replace(".000000", "")
        .replace("+00:00", "Z")
}
fn moment(value: &Value) -> Option<f64> {
    value.as_str().and_then(target::seconds)
}
fn queued(reason: &str, at: f64) -> Value {
    json!({"kind":"queued","reason":reason,"eta":stamp(at)})
}
fn private(text: &str, cfg: &DocumentMut) -> bool {
    let (item, _, needs) = facets(text);
    let repo = item
        .split('#')
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    needs.contains("private")
        || needs.contains("local-state")
        || strings(cfg.get("placement").and_then(|p| p.get("private_repos"))).contains(&repo)
}
type Placed = (String, String);

fn place_anthropic_cloud(cfg: &DocumentMut, at: f64, probe: &Value) -> Result<Placed, String> {
    let (usable, line) = if let Some(pair) = probe["credit"].as_array() {
        (
            pair[0].as_bool().unwrap_or(false),
            pair[1].as_str().unwrap_or_default().to_owned(),
        )
    } else {
        credit(cfg, at)
    };
    if usable {
        return Ok(("anthropic-cloud".into(), line));
    }
    Err(format!(
        "anthropic-cloud {}",
        line.trim_start_matches("cloud credit: ")
    ))
}
fn place_laptop(
    code_only: bool,
    probe: &Value,
    laptop_why: &mut Option<String>,
) -> Result<Placed, String> {
    if code_only {
        return Err("laptop refused for code-only work: fix the cloud or remote lane".into());
    }
    let state = probe["laptop"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(laptop);
    if state == "available" {
        return Ok(("laptop".into(), "laptop available".into()));
    }
    let reason = format!("laptop {state}");
    *laptop_why = Some(reason.clone());
    Err(reason)
}
fn place_resident(cfg: &DocumentMut, probe: &Value, name: &str) -> Option<Result<Placed, String>> {
    let cap = cfg
        .get("hosts")
        .and_then(|h| h.get(name))
        .and_then(|h| h.get("resident_cap"))
        .and_then(Item::as_integer)?;
    let (online, live) = if let Some(pair) = probe[name].as_array() {
        (
            pair[0].as_bool().unwrap_or(false),
            pair[1].as_i64().unwrap_or(0),
        )
    } else {
        let ssh = cfg
            .get("hosts")
            .and_then(|h| h.get(name))
            .and_then(|h| h.get("ssh"))
            .and_then(Item::as_str)
            .unwrap_or(name);
        resident(name, ssh).map_or((false, 0), |live| (true, live))
    };
    if online && live < cap {
        return Some(Ok((name.to_owned(), format!("{name} {live}/{cap}"))));
    }
    Some(Err(if online {
        format!("{name} {live}/{cap}")
    } else {
        format!("{name} offline")
    }))
}
fn skipped_target(name: &str, code_only: bool, native: bool) -> bool {
    name == "anthropic-cloud" && !code_only || native && name != "laptop"
}
fn held_placement(laptop_why: Option<String>, why: Vec<String>) -> Placed {
    (
        format!(
            "held: {}",
            laptop_why
                .or_else(|| why.first().cloned())
                .unwrap_or_else(|| "no target in [placement] order".into())
        ),
        why.join("; "),
    )
}
fn place(text: &str, cfg: &DocumentMut, at: f64, probe: &mut Value) -> Placed {
    let (_, native, _) = facets(text);
    let code_only = !native && !private(text, cfg);
    let rules = cfg.get("placement");
    let mut why = Vec::new();
    let mut laptop_why = None;
    for name in strings(rules.and_then(|p| p.get("order"))) {
        if skipped_target(&name, code_only, native) {
            continue;
        }
        let attempt = match name.as_str() {
            "anthropic-cloud" => Some(place_anthropic_cloud(cfg, at, probe)),
            "laptop" => Some(place_laptop(code_only, probe, &mut laptop_why)),
            _ => place_resident(cfg, probe, &name),
        };
        match attempt {
            Some(Ok(found)) => return found,
            Some(Err(reason)) => why.push(reason),
            None => {}
        }
    }
    held_placement(laptop_why, why)
}
fn release(reason: &str, control: &Value, at: f64) -> Vec<f64> {
    match reason {
        "short-window" => rows(&control["windows"])
            .iter()
            .filter(|w| w["window"] != "week")
            .filter_map(|w| moment(&w["resets_at"]))
            .collect(),
        "floor" | "blocked" | "" => Vec::new(),
        "workers" => vec![(at + 60.0).max(
            number(&control["state_since"], at) + storage::default("pace_state_dwell_min") * 60.0,
        )],
        _ => vec![at + 60.0],
    }
}
fn fresh(raw: &Value, at: f64) -> bool {
    serde_json::from_value::<Control>(raw.clone()).is_ok_and(|c| admission::is_fresh(&c, at))
}
fn minimum(values: &[f64], at: f64) -> f64 {
    values
        .iter()
        .copied()
        .min_by(f64::total_cmp)
        .unwrap_or(at + 60.0)
}
pub(super) fn operation(request: &Value) -> Result<Option<Value>, String> {
    let at = request["now"]
        .as_str()
        .and_then(target::seconds)
        .unwrap_or_else(storage::now);
    Ok(Some(match request["op"].as_str().unwrap_or_default() {
        "place_spawn" => {
            let cfg = request["config"]
                .as_str()
                .ok_or("config TOML required")?
                .parse::<DocumentMut>()
                .map_err(|e| e.to_string())?;
            spawn(
                request["brief"].as_str().ok_or("brief required")?,
                &request["spawner"],
                &request["gate"],
                &cfg,
                at,
                Probes {
                    configured: request["probe"].clone(),
                    observed: request["observations"].clone(),
                },
                Routing {
                    local: &local(),
                    defaults: &routing_defaults(),
                    account_order: &request["account_order"],
                },
            )
        }
        "admit" => admit(request, at)?,
        _ => return Ok(None),
    }))
}
pub(super) fn cli(args: &[String], roster: fn() -> NorthResult<Roster>) -> Result<(), String> {
    let cfg = sampling::config()?;
    let at = std::env::var("PLACEMENT_NOW")
        .ok()
        .and_then(|s| target::seconds(&s))
        .unwrap_or_else(storage::now);
    if args == ["credit"] {
        println!("{}", credit(&cfg, at).1);
        return Ok(());
    }
    if args.first().is_none_or(|s| s != "place") || args.len() < 2 {
        return Err("north place place BRIEF... | credit".into());
    }
    let gate = storage::load(&storage::directory().join("usage-gate.json"));
    let machines = gate::probed(&cfg, &Default::default());
    let roster = roster().map_err(|error| error.to_string())?;
    for path in &args[1..] {
        let text = if path == "-" {
            use std::io::Read;
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| e.to_string())?;
            text
        } else {
            std::fs::read_to_string(path).map_err(|e| e.to_string())?
        };
        let decision = gate::decide(&text, &cfg, gate::verdicts(&gate, &roster, at), credit(&cfg, at), &machines, &local());
        for line in decision.verdicts.iter().map(|v| &v.line).chain(&decision.machine_lines) {
            println!("  {line}");
        }
        let item = field(&text, "Item");
        println!(
            "{}: {}",
            if item.is_empty() { path } else { item },
            gate::summary(&decision)
        );
    }
    Ok(())
}
