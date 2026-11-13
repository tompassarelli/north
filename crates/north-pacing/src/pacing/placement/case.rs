//! Placement's case file: every input the live decision reads, gathered into one `Case` before it
//! decides, and the decision taken from that `Case` alone, so the decision log can hold it and replay
//! can run it again (docs/rationale/strategy-kernel.md).
use super::super::storage;
use super::gate::{self, Decision, Machine};
use super::credit;
use super::hosts::{self, Facts};
use super::line_wait;
use super::room::{Basis, Rank};
use super::{local, strings};
use crate::capacity::saturation::Saturation;
use crate::capacity::{agent_load, laptop_auto, mode};
use crate::fleet::roster::Roster;
use crate::usage::decide::world::ModelFamily;
use crate::usage::meter;
use north_strategy_types::placement::{Case, Config, GrantSource, Here, Host, Launch, Usage, Window};
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

/// Reads every input placement decides from, once, at one clock reading, parsing the model catalog and
/// the harness defaults here so the timed decision spends none of its `decide_us` on that.
pub fn gather(ask: &Ask) -> Result<Case, String> {
    for table in [crate::platform::harness::MODEL_CATALOG, crate::platform::harness::DEFAULTS] {
        crate::platform::harness::document(table);
    }
    let text = std::fs::read_to_string(crate::platform::orchestration::path()).map_err(|error| error.to_string())?;
    let cfg: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
    let at = std::env::var("PLACEMENT_NOW").ok().and_then(|s| storage::seconds(&s)).unwrap_or_else(storage::now);
    let (gate, usage) = usage(at);
    let unprobed = gate::machines(&cfg);
    super::keeper::wanted();
    let surveyed = hosts::surveyed(&unprobed, ask.launch.cwd.as_deref());
    let machines = unprobed.iter().zip(surveyed).map(|(machine, facts)| recorded(machine, facts, ask.fleet_load)).collect();
    let credit_name = credit::credit_name(&cfg);
    Ok(Case {
        at,
        launch: ask.launch.clone(),
        brief: ask.brief.to_owned(),
        config: config(&cfg, crate::platform::decision_log::sha256_hex(text.as_bytes())),
        credit: credit::credit_case(&credit_name),
        roster: gate::standings(&gate, ask.roster),
        usage,
        machines,
        here: here(),
        github: crate::platform::github_quota::seen(at),
        grants: grant_sources(ask.roster, at),
        line_waits: if line_wait::counts(ask.launch.role.as_deref()) { hosts::surveyed_lines(&cfg, &local(), unprobed) } else { Vec::new() },
    })
}

/// The live decision from the case alone: the same steps `gate::live` always took, on recorded inputs.
pub fn decide(case: &Case) -> Result<Decision, String> {
    let cfg = document(&case.config);
    let gate_value = gate_value(&case.usage);
    let hosted: BTreeMap<String, i64> = case.machines.iter().map(|machine| (machine.name.clone(), machine.hosted)).collect();
    let machines: Vec<Machine> = gate::machines(&cfg).into_iter().map(|machine| {
        let recorded = case.machines.iter().find(|probed| probed.name == machine.name);
        let placed_recently = recorded.map_or(0, |probed| probed.placed_recently);
        Machine { placed_recently, ..machine.probed(recorded.map(facts).unwrap_or_default(), &hosted) }
    }).collect();
    let basis = Basis::new(&cfg, case.at);
    let credit = credit::credit_verdict(&case.credit, case.config.cloud_credit.as_deref().unwrap_or_default(), case.at);
    let cloud_unrecorded = credit.0 && case.machines.iter().any(|machine| machine.claude_ai.is_none());
    let here = case.here.name.as_str();
    let family = launch_family(&case.launch);
    let coordinator = is_coordinator(&case.launch);
    let mut verdicts = gate::verdicts_of_for(&gate_value, &case.roster, &basis, Some(family));
    if coordinator {
        rank_by_headroom(&mut verdicts);
    }
    let fable_has_room = has_headroom(&verdicts);
    verdicts.extend(case.grants.iter().map(|source| gate::grant_verdict(source, basis.operational(&source.account))));
    let mut decision = gate::decide(&case.brief, &cfg, verdicts, credit.clone(), &machines, here, case.here.gaming);
    decision.family = Some(family);
    if coordinator && family == ModelFamily::Fable && !fable_has_room {
        let mut opus = gate::verdicts_of_for(&gate_value, &case.roster, &basis, Some(ModelFamily::Opus));
        rank_by_headroom(&mut opus);
        if opus.iter().any(gate::has_floor_headroom) {
            opus.extend(case.grants.iter().map(|source| gate::grant_verdict(source, basis.operational(&source.account))));
            decision = gate::decide(&case.brief, &cfg, opus, credit, &machines, here, case.here.gaming);
            decision.family = Some(ModelFamily::Opus);
            decision.machine_lines.push("no account has Fable headroom; the coordinator starts on Opus high".to_owned());
        }
    }
    if cloud_unrecorded && decision.code_only {
        return Err("the case predates the claude.ai sign-in record, so the cloud lane it would consult cannot be replayed".to_owned());
    }
    decision.here_on_battery = case.here.battery.map(|percent| gate::battery_refusal(here, percent, &machines));
    decision.here_closed = gate::closed(here, case.here.playtest_hold_until, || case.here.battery, case.at as u64);
    if let Some(name) = case.launch.named_machine.as_deref() {
        let machine = machines.into_iter().find(|machine| machine.name == name).ok_or_else(|| format!("no [hosts.{name}] with an ssh destination"))?;
        decision.run_on(machine);
    }
    decision.here_saturated = case.here.saturation.map(Saturation::from).and_then(|reading| reading.refusal(here));
    decision.here_cpu_held = case.here.spawn_cpu.as_ref().and_then(|cpu| crate::pacing::spawn_gate::launch_verdict(cpu, &case.brief));
    decision.hold_here();
    decision.hold_for_line(case);
    Ok(decision)
}

impl Decision {
    /// Holds a worker's launch while every build host's line is over the bar; only a cloud placement escapes it, and `--local` keeps the box off the cloud.
    /// A launch the coordinator granted urgent priority passes the hold and the pass is recorded in the decision's lines.
    pub fn hold_for_line(&mut self, case: &Case) {
        let on_cloud = matches!(self.target, gate::Target::Cloud(_)) && case.launch.overrides.local.is_none();
        if !line_wait::counts(case.launch.role.as_deref()) || on_cloud {
            return;
        }
        let Some(why) = line_wait::over_the_bar(&case.line_waits, line_wait::bar_s()) else { return };
        if case.launch.overrides.urgent {
            self.machine_lines.push(format!("urgent priority passes the hold: {why}"));
            return;
        }
        self.machine_lines.push(why.clone());
        self.line_held = Some(why);
    }
}

/// The model family a launch asks placement to meter: the family its recorded model names, else the
/// shared Opus windows it has always used.
pub fn launch_family(launch: &Launch) -> ModelFamily {
    launch.model.as_deref().and_then(meter::family).unwrap_or(ModelFamily::Opus)
}

/// Whether the launch is the one fleet coordinator, however the role is spelled in an older record.
fn is_coordinator(launch: &Launch) -> bool {
    matches!(launch.role.as_deref(), Some("proxy" | "coordinator"))
}

fn has_headroom(verdicts: &[gate::Verdict]) -> bool {
    verdicts.iter().any(gate::has_floor_headroom)
}

fn rank_by_headroom(verdicts: &mut [gate::Verdict]) {
    for verdict in verdicts.iter_mut().filter(|verdict| verdict.available) {
        verdict.rank = Rank::headroom(verdict.headroom);
    }
}

fn recorded(machine: &Machine, facts: Facts, fleet_load: &BTreeMap<String, i64>) -> north_strategy_types::placement::Machine {
    north_strategy_types::placement::Machine {
        name: machine.name.clone(),
        live: facts.live,
        accounts: facts.accounts,
        claude_ai: Some(facts.claude_ai),
        saturation: facts.saturation.map(Into::into),
        saturation_age_s: facts.saturation_age_s,
        stale_age_s: facts.stale_age_s,
        hosted: fleet_load.get(&machine.name).copied().unwrap_or(0),
        placed_recently: 0,
        has_cwd: facts.has_cwd,
    }
}

fn facts(machine: &north_strategy_types::placement::Machine) -> Facts {
    Facts { live: machine.live, accounts: machine.accounts.clone(), claude_ai: machine.claude_ai.clone().unwrap_or_default(), saturation: machine.saturation.map(Into::into), saturation_age_s: machine.saturation_age_s, has_cwd: machine.has_cwd, stale_age_s: machine.stale_age_s }
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

/// The Claude accounts' sampled windows in usage-gate.json, with the file's sha256 and age, and the gate as decisions read it.
fn usage(at: f64) -> (Value, Usage) {
    let path = gate::live_gate_path();
    let bytes = std::fs::read(&path).unwrap_or_default();
    let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
    let gate: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({}));
    let controls = gate["claude"]["windows"].as_object().into_iter().flatten().filter(|(name, _)| name.starts_with("claude:")).map(|(name, sampled)| (name.clone(), storage::rows(sampled).iter().map(window).collect())).collect();
    let usage = Usage { sha256: crate::platform::decision_log::sha256_hex(&bytes), age_s: modified.map(|since| at - since.as_secs_f64()), controls };
    (gate_value(&usage), usage)
}

fn window(window: &Value) -> Window {
    let text = |key: &str| window[key].as_str().map(str::to_owned);
    Window { window: text("window"), used_pct: window["used_pct"].as_f64(), resets_at: text("resets_at"), reset_h: window["reset_h"].as_f64() }
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
                    Value::Object(fields)
                })
                .collect();
            (name.clone(), Value::Array(windows))
        })
        .collect();
    json!({ "claude": { "windows": controls } })
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

/// The target as the decision log records it.
pub fn recorded_target(target: &gate::Target) -> north_strategy_types::placement::Target {
    use north_strategy_types::placement::Target as Recorded;
    match target {
        gate::Target::Cloud(lane) => Recorded::Cloud { account: lane.account.clone(), host: lane.host.clone(), credit: lane.credit.clone() },
        gate::Target::Account(account) => Recorded::Account(account.clone()),
        gate::Target::Held(why) => Recorded::Held(why.clone()),
        gate::Target::Unserved(why) => Recorded::Unserved(why.clone()),
    }
}

/// Where placement targets the launch a logged case recorded, decided again from the case alone.
pub fn replayed_target(case: &Case) -> Option<north_strategy_types::placement::Target> {
    decide(case).ok().map(|decision| recorded_target(&decision.target))
}

/// The API grants this host can launch on (its roster lists `api:NAME`) that are unexpired, unmarked and have credit left.
pub fn grant_sources(roster: &Roster, at: f64) -> Vec<GrantSource> {
    grants_left(at).into_iter().filter(|source| keyed_here(roster, &source.account)).collect()
}

/// Whether this host's roster holds a key for the API grant account `account` (`claude:api:NAME`).
pub fn keyed_here(roster: &Roster, account: &str) -> bool {
    let claude = crate::fleet::model::ProviderName::new("claude");
    matches!(roster.standing(&claude, account.trim_start_matches("claude:")), crate::fleet::roster::Standing::Listed(_))
}

/// Every API grant with credit left and no exhausted mark, wherever its keys are held.
pub fn grants_left(at: f64) -> Vec<GrantSource> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), std::path::PathBuf::from);
    let now = crate::foundation::clock::Timestamp::from_unix_seconds_f64(at);
    crate::credit::sources(&crate::credit::standing(&crate::platform::orchestration::path(), &state, now))
        .into_iter()
        .filter(|source| !source.credit.exhausted && source.credit.left > 0.0)
        .map(|source| {
            let now_s = now.unix_seconds();
            GrantSource { account: format!("claude:api:{}", source.credit.name), left_usd: source.credit.left, deadline_h: source.deadline_h(now_s), urgent: !source.drains(now_s), line: source.line(now_s) }
        })
        .collect()
}

/// Every plan account and API grant this host can launch on, in the order placement ranks them (first is
/// placed first), each with its verdict line and the reason for its rank: what `north fleet accounts --plan` prints.
pub fn plan(roster: &Roster) -> Result<Vec<(String, String)>, String> {
    Ok(verdicts(roster)?.into_iter().map(|verdict| (verdict.line, if verdict.available { verdict.rank.reason() } else { "unavailable".to_owned() })).collect())
}

/// Every plan account as placement judges it from the usage gate file the sampler wrote; no provider is polled.
pub fn accounts(roster: &Roster) -> Result<Vec<gate::Verdict>, String> {
    let text = std::fs::read_to_string(crate::platform::orchestration::path()).map_err(|error| error.to_string())?;
    let cfg: DocumentMut = text.parse().map_err(|error: toml_edit::TomlError| error.to_string())?;
    let at = storage::now();
    let (gate_value, _) = usage(at);
    let basis = Basis::new(&cfg, at);
    Ok(gate::verdicts(&gate_value, roster, &basis).into_iter().filter(|verdict| verdict.owners.is_empty()).collect())
}

/// The plan account run here with the most headroom that placement keeps open to workers: no reserve or hold names it.
pub fn roomiest(roster: &Roster) -> Result<Option<String>, String> {
    Ok(gate::roomiest_account(&accounts(roster)?))
}

/// Every account and api grant as placement judges it, the available ones first in placement order.
pub fn verdicts(roster: &Roster) -> Result<Vec<gate::Verdict>, String> {
    let at = storage::now();
    let cfg = crate::platform::orchestration::read()?;
    let basis = Basis::new(&cfg, at);
    let mut verdicts = accounts(roster)?;
    let grants = if crate::spend::hold::declared(&cfg).is_some() { grants_left(at) } else { grant_sources(roster, at) };
    verdicts.extend(grants.iter().map(|source| gate::grant_verdict(source, basis.operational(&source.account))));
    let reserved = crate::spend::operational_reserve(&cfg).into_iter().filter(|account| keyed_here(roster, account) && !verdicts.iter().any(|verdict| &verdict.account == account));
    let reserved: Vec<_> = reserved.filter_map(|account| basis.operational(&account).map(|why| gate::reserve_verdict(&account, why))).collect();
    verdicts.extend(reserved);
    verdicts.sort_by(|a, b| b.available.cmp(&a.available).then(b.rank.order(&a.rank)).then(a.account.cmp(&b.account)));
    Ok(verdicts)
}
