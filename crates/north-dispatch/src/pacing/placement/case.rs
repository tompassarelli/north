//! Placement's case file: every input the live decision reads, gathered into one `Case` before it
//! decides, and the decision taken from that `Case` alone, so the decision log can hold it and replay
//! can run it again (docs/rationale/strategy-kernel.md).
use super::super::storage::{self, load};
use super::super::{sampling, target};
use super::gate::{self, Decision, Machine};
use super::hosts::{self, Facts};
use super::room::Basis;
use super::{local, strings};
use crate::capacity::saturation::Saturation;
use crate::capacity::{agent_load, laptop_auto, mode};
use crate::fleet::roster::Roster;
use north_strategy_types::placement::{Case, Config, Here, Host, Launch, Usage, Window};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use toml_edit::{Array, DocumentMut, Item, Table, value};

/// What a launch asks placement: the launch, its brief as placement parses it, and the fleet sessions
/// this machine runs on each other machine.
pub struct Ask<'a> {
    pub launch: Launch,
    pub brief: &'a str,
    pub fleet_load: &'a BTreeMap<String, i64>,
    pub roster: &'a Roster,
}

/// Reads every input placement decides from, once, at one clock reading.
pub fn gather(ask: &Ask) -> Result<Case, String> {
    let text = std::fs::read_to_string(sampling::policy_path()).map_err(|error| error.to_string())?;
    let cfg: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
    let at = std::env::var("PLACEMENT_NOW").ok().and_then(|s| target::seconds(&s)).unwrap_or_else(storage::now);
    let (gate, usage) = usage(at);
    let unprobed = gate::machines(&cfg);
    let surveyed = hosts::surveyed(&unprobed, ask.launch.cwd.as_deref());
    let machines = unprobed.iter().zip(surveyed).map(|(machine, facts)| recorded(machine, facts, ask.fleet_load)).collect();
    let credit_name = hosts::credit_name(&cfg);
    Ok(Case {
        at,
        launch: ask.launch.clone(),
        brief: ask.brief.to_owned(),
        config: config(&cfg, crate::platform::decision_log::sha256_hex(text.as_bytes())),
        credit: hosts::credit_case(&credit_name),
        pace: targets(&load(&storage::directory().join("pace-control.json"))),
        roster: gate::standings(&gate, ask.roster),
        usage,
        machines,
        here: here(),
    })
}

/// The live decision from the case alone: the same steps `gate::live` always took, on recorded inputs.
pub fn decide(case: &Case) -> Result<Decision, String> {
    let cfg = document(&case.config);
    let gate_value = gate_value(&case.usage);
    let hosted: BTreeMap<String, i64> = case.machines.iter().map(|machine| (machine.name.clone(), machine.hosted)).collect();
    let machines: Vec<Machine> = gate::machines(&cfg).into_iter().map(|machine| {
        let facts = case.machines.iter().find(|probed| probed.name == machine.name).map(facts).unwrap_or_default();
        machine.probed(facts, &hosted)
    }).collect();
    let basis = Basis::new(&cfg, pace_value(&case.pace), case.at);
    let credit = hosts::credit_verdict(&case.credit, case.config.cloud_credit.as_deref().unwrap_or_default(), case.at);
    let here = case.here.name.as_str();
    let mut decision = gate::decide(&case.brief, &cfg, gate::verdicts_of(&gate_value, &case.roster, &basis), credit, &machines, here, case.here.gaming);
    decision.here_on_battery = case.here.battery.map(|percent| gate::battery_refusal(here, percent, &machines));
    decision.here_closed = gate::closed(here, case.here.playtest_hold_until, || case.here.battery, case.at as u64);
    if let Some(name) = case.launch.named_machine.as_deref() {
        let machine = machines.into_iter().find(|machine| machine.name == name).ok_or_else(|| format!("no [hosts.{name}] with an ssh destination"))?;
        decision.run_on(machine);
    }
    decision.here_saturated = case.here.saturation.map(Saturation::from).and_then(|reading| reading.refusal(here));
    decision.here_cpu_held = case.here.spawn_cpu.as_ref().and_then(|cpu| crate::pacing::spawn_gate::launch_verdict(cpu, &case.brief));
    decision.hold_here();
    Ok(decision)
}

fn recorded(machine: &Machine, facts: Facts, fleet_load: &BTreeMap<String, i64>) -> north_strategy_types::placement::Machine {
    north_strategy_types::placement::Machine {
        name: machine.name.clone(),
        live: facts.live,
        accounts: facts.accounts,
        saturation: facts.saturation.map(Into::into),
        saturation_age_s: facts.saturation_age_s,
        hosted: fleet_load.get(&machine.name).copied().unwrap_or(0),
        has_cwd: facts.has_cwd,
    }
}

fn facts(machine: &north_strategy_types::placement::Machine) -> Facts {
    Facts { live: machine.live, accounts: machine.accounts.clone(), saturation: machine.saturation.map(Into::into), saturation_age_s: machine.saturation_age_s, has_cwd: machine.has_cwd }
}

fn here() -> Here {
    let host = agent_load::Host::from_env();
    Here {
        name: local(),
        gaming: gate::here_in_game_reserve(),
        battery: laptop_auto::settled_battery(&laptop_auto::power_supply(), &laptop_auto::discharging_since(&host), mode::now()),
        playtest_hold_until: mode::read(&mode::state_dir()).hold.map(|hold| hold.until),
        saturation: hosts::saturation(&local(), "").map(Into::into),
        spawn_cpu: hosts::here_spawn_cpu(),
    }
}

/// The Claude controls in usage-gate.json, with the file's sha256 and age, and the gate as decisions read it.
fn usage(at: f64) -> (Value, Usage) {
    let path = gate::live_gate_path();
    let bytes = std::fs::read(&path).unwrap_or_default();
    let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
    let gate: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    let controls = gate["claude"]["controls"].as_object().into_iter().flatten().filter(|(name, _)| name.starts_with("claude:")).map(|(name, control)| (name.clone(), storage::rows(&control["windows"]).iter().map(window).collect())).collect();
    let usage = Usage { sha256: crate::platform::decision_log::sha256_hex(&bytes), age_s: modified.map(|since| at - since.as_secs_f64()), controls };
    (gate_value(&usage), usage)
}

fn window(window: &Value) -> Window {
    let text = |key: &str| window[key].as_str().map(str::to_owned);
    Window { window: text("window"), used_pct: window["used_pct"].as_f64(), resets_at: text("resets_at"), reset_h: window["reset_h"].as_f64(), burn_pct_h: window["burn_pct_h"].as_f64() }
}

fn gate_value(usage: &Usage) -> Value {
    let controls: Map<String, Value> = usage
        .controls
        .iter()
        .map(|(name, windows)| {
            let windows: Vec<Value> = windows
                .iter()
                .map(|window| {
                    let mut fields = Map::new();
                    let mut put = |key: &str, field: Option<Value>| {
                        if let Some(field) = field {
                            fields.insert(key.to_owned(), field);
                        }
                    };
                    put("window", window.window.clone().map(Value::from));
                    put("used_pct", window.used_pct.map(Value::from));
                    put("resets_at", window.resets_at.clone().map(Value::from));
                    put("reset_h", window.reset_h.map(Value::from));
                    put("burn_pct_h", window.burn_pct_h.map(Value::from));
                    Value::Object(fields)
                })
                .collect();
            (name.clone(), json!({ "windows": windows }))
        })
        .collect();
    json!({ "claude": { "controls": controls } })
}

/// `accounts.NAME.target.by` for each account pace-control holds one for: the one field placement reads of that file.
fn targets(pace: &Value) -> BTreeMap<String, String> {
    pace["accounts"].as_object().into_iter().flatten().filter_map(|(name, account)| Some((name.clone(), account["target"]["by"].as_str()?.to_owned()))).collect()
}

fn pace_value(targets: &BTreeMap<String, String>) -> Value {
    let accounts: Map<String, Value> = targets.iter().map(|(name, by)| (name.clone(), json!({ "target": { "by": by } }))).collect();
    json!({ "accounts": accounts })
}

fn config(cfg: &DocumentMut, sha256: String) -> Config {
    let placement = cfg.get("placement");
    let listed = |key: &str| strings(placement.and_then(|p| p.get(key)));
    let hosts = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter()).map(|(name, host)| Host {
        name: name.to_owned(),
        ssh: host.get("ssh").and_then(Item::as_str).map(str::to_owned),
        serves: strings(host.get("serves")),
        pushes: host.get("pushes").map(|_| strings(host.get("pushes"))),
        reaches: host.get("reaches").map(|_| strings(host.get("reaches"))),
        resident_cap: host.get("resident_cap").and_then(Item::as_integer),
        launches: host.get("launches").and_then(Item::as_bool) == Some(true),
    });
    let plans = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("plans")).and_then(Item::as_table_like);
    Config {
        sha256,
        order: listed("order"),
        public_repos: listed("public_repos"),
        private_repos: listed("private_repos"),
        cloud_credit: placement.and_then(|p| p.get("cloud_credit")).and_then(Item::as_str).map(str::to_owned),
        hosts: hosts.collect(),
        plans: plans.into_iter().flat_map(|plans| plans.iter()).filter_map(|(label, plan)| Some((label.to_owned(), plan.as_str()?.to_owned()))).collect(),
    }
}

/// The orchestration document the recorded config stands for, holding exactly the keys placement reads.
fn document(config: &Config) -> DocumentMut {
    let array = |items: &[String]| Item::Value(Array::from_iter(items.iter().map(String::as_str)).into());
    let mut placement = Table::new();
    placement.insert("order", array(&config.order));
    placement.insert("public_repos", array(&config.public_repos));
    placement.insert("private_repos", array(&config.private_repos));
    if let Some(credit) = &config.cloud_credit {
        placement.insert("cloud_credit", value(credit.as_str()));
    }
    let mut hosts = Table::new();
    for host in &config.hosts {
        let mut table = Table::new();
        if let Some(ssh) = &host.ssh {
            table.insert("ssh", value(ssh.as_str()));
        }
        table.insert("serves", array(&host.serves));
        if let Some(pushes) = &host.pushes {
            table.insert("pushes", array(pushes));
        }
        if let Some(reaches) = &host.reaches {
            table.insert("reaches", array(reaches));
        }
        if let Some(cap) = host.resident_cap {
            table.insert("resident_cap", value(cap));
        }
        table.insert("launches", value(host.launches));
        hosts.insert(&host.name, Item::Table(table));
    }
    let mut plans = Table::new();
    for (label, plan) in &config.plans {
        plans.insert(label, value(plan.as_str()));
    }
    let mut claude = Table::new();
    claude.insert("plans", Item::Table(plans));
    let mut accounts = Table::new();
    accounts.insert("claude", Item::Table(claude));
    let mut document = DocumentMut::new();
    document.insert("placement", Item::Table(placement));
    document.insert("hosts", Item::Table(hosts));
    document.insert("accounts", Item::Table(accounts));
    document
}

#[cfg(test)]
#[path = "case_tests.rs"]
mod tests;
