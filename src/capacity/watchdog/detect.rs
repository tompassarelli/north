use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

pub(super) const COORDINATOR: &str = "proxy";
const SESSION_PREFIX: &str = "north-session-";
const LEASE_PREFIX: &str = "agent-capacity-";
const TREES_NAMED: usize = 5;
const UNOWNED_NAMED: usize = 3;
const LEASE_SCOPE_ID_LENGTH: usize = 32;
const MINIMUM_QUOTA_PERCENT: f64 = 100.0;
const CAPPED_WEIGHT: u32 = 10;

static DEFAULTS: std::sync::LazyLock<DocumentMut> = std::sync::LazyLock::new(|| include_str!("../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

fn registry(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Settings {
    pub(super) psi10: f64,
    pub(super) psi60: f64,
    pub(super) load_per_core: f64,
    pub(super) sustained_samples: u32,
    pub(super) calm_psi60: f64,
    pub(super) offender_cores: f64,
    pub(super) cap_share: f64,
    pub(super) cpus: f64,
}

impl Settings {
    pub(super) fn bundled(cpus: f64) -> Self {
        Self {
            psi10: registry("overload_psi10"),
            psi60: registry("overload_psi60"),
            load_per_core: registry("overload_load_per_core"),
            sustained_samples: registry("overload_sustained_samples") as u32,
            calm_psi60: registry("overload_calm_psi60"),
            offender_cores: registry("overload_offender_cores"),
            cap_share: registry("overload_cap_share"),
            cpus,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub(super) struct Owner {
    pub(super) session: String,
    #[serde(default)]
    pub(super) parent: Option<String>,
}

pub(super) trait Owners {
    fn owner(&mut self, unit: &str) -> Option<Owner>;
}

impl Owners for BTreeMap<String, Owner> {
    fn owner(&mut self, unit: &str) -> Option<Owner> {
        self.get(unit).cloned()
    }
}

pub(super) struct Sample {
    pub(super) at: String,
    pub(super) load1: f64,
    pub(super) psi10: f64,
    pub(super) psi60: f64,
    pub(super) cpus: Option<f64>,
    cgroups: Vec<(String, f64)>,
    tops: Vec<(String, f64, String)>,
}

impl Sample {
    pub(super) fn parse(value: &Value) -> Option<Self> {
        let cgroups = value["cgroups"].as_array()?.iter().filter_map(|entry| Some((entry["cgroup"].as_str()?.trim_start_matches('/').to_owned(), entry["cores"].as_f64()?))).collect();
        let tops = value["top"].as_array().map_or_else(Vec::new, |tops| {
            tops.iter().filter_map(|top| Some((top["cgroup"].as_str()?.trim_start_matches('/').to_owned(), top["cores"].as_f64()?, top["command"].as_str()?.to_owned()))).collect()
        });
        Some(Self { at: value["at"].as_str()?.to_owned(), load1: value["load"][0].as_f64()?, psi10: value["cpuPsi"].as_f64()?, psi60: value["cpuPsi60"].as_f64()?, cpus: value["cpus"].as_f64(), cgroups, tops })
    }

    fn lead_command(&self, cgroup: &str) -> Option<String> {
        self.tops.iter().filter(|(owner, ..)| owner == cgroup).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(.., command)| command.chars().take(100).collect())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Cap {
    pub(super) unit: String,
    pub(super) session: Option<String>,
    pub(super) cpu_quota_percent: u32,
    pub(super) cpu_weight: u32,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(super) struct State {
    over: u32,
    calm: u32,
    reported: bool,
    cores: BTreeMap<String, f64>,
    pub(super) caps: Vec<Cap>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Tree {
    pub(super) unit: String,
    pub(super) cores: f64,
    pub(super) kind: &'static str,
    pub(super) session: Option<String>,
    pub(super) parent: Option<String>,
    pub(super) lead: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct Incident {
    pub(super) at: String,
    pub(super) load1: f64,
    pub(super) load_per_core: f64,
    pub(super) psi60: f64,
    pub(super) trees: Vec<Tree>,
    pub(super) capped: Option<Cap>,
}

#[derive(Debug)]
pub(super) enum Action {
    Incident(Incident),
    Cap(Cap),
    Release(Cap),
    Send { to: String, text: String },
}

fn unit_of(cgroup: &str) -> &str {
    cgroup.rsplit('/').next().unwrap_or(cgroup)
}

fn fleet_scope(cgroup: &str) -> bool {
    cgroup.strip_prefix("agent.slice/").is_some_and(|unit| unit.starts_with(SESSION_PREFIX) && unit.ends_with(".scope") && !unit.contains('/'))
}

fn kind(cgroup: &str) -> &'static str {
    let unit = unit_of(cgroup);
    if fleet_scope(cgroup) {
        "session"
    } else if unit.starts_with(LEASE_PREFIX) && unit.len() >= LEASE_PREFIX.len() + LEASE_SCOPE_ID_LENGTH {
        "lease"
    } else {
        "other"
    }
}

fn is_unit(cgroup: &str) -> bool {
    let unit = unit_of(cgroup);
    unit.ends_with(".scope") || unit.ends_with(".service")
}

pub(super) fn step(settings: &Settings, owners: &mut dyn Owners, mut state: State, sample: &Sample) -> (State, Vec<Action>) {
    let cpus = sample.cpus.unwrap_or(settings.cpus).max(1.0);
    let load_per_core = sample.load1 / cpus;
    let over = sample.psi10 >= settings.psi10 && sample.psi60 >= settings.psi60 && load_per_core >= settings.load_per_core;
    let mut actions = Vec::new();
    if over {
        state.over += 1;
        state.calm = 0;
        for (cgroup, cores) in &sample.cgroups {
            let peak = state.cores.entry(cgroup.clone()).or_default();
            *peak = peak.max(*cores);
        }
    } else {
        state.calm = if sample.psi60 < settings.calm_psi60 { state.calm + 1 } else { 0 };
        if !state.reported {
            state.over = 0;
            state.cores.clear();
        }
    }
    if over && !state.reported && state.over >= settings.sustained_samples {
        state.reported = true;
        actions.extend(report(settings, owners, &mut state, sample, load_per_core));
    }
    if state.reported && state.calm >= settings.sustained_samples {
        for cap in std::mem::take(&mut state.caps) {
            if let Some(session) = cap.session.clone() {
                actions.push(Action::Send { to: session, text: format!("watchdog: host load is back down; your scope {} is no longer capped.", cap.unit) });
            }
            actions.push(Action::Release(cap));
        }
        state = State::default();
    }
    (state, actions)
}

fn report(settings: &Settings, owners: &mut dyn Owners, state: &mut State, sample: &Sample, load_per_core: f64) -> Vec<Action> {
    let ranked = ranked(&state.cores);
    let (owned, unowned): (Vec<Tree>, Vec<Tree>) = ranked
        .iter()
        .map(|(cgroup, cores)| {
            let owner = owners.owner(unit_of(cgroup));
            Tree { unit: unit_of(cgroup).to_owned(), cores: round(*cores), kind: kind(cgroup), session: owner.as_ref().map(|owner| owner.session.clone()), parent: owner.and_then(|owner| owner.parent), lead: sample.lead_command(cgroup) }
        })
        .partition(|tree| tree.session.is_some());
    let trees: Vec<Tree> = owned.into_iter().take(TREES_NAMED).chain(unowned.into_iter().take(UNOWNED_NAMED)).collect();
    let capped = ranked.iter().find(|(cgroup, cores)| fleet_scope(cgroup) && *cores >= settings.offender_cores).map(|(cgroup, cores)| {
        let owner = owners.owner(unit_of(cgroup));
        Cap {
            unit: unit_of(cgroup).to_owned(),
            session: owner.map(|owner| owner.session),
            cpu_quota_percent: (cores * settings.cap_share * 100.0).max(MINIMUM_QUOTA_PERCENT).round() as u32,
            cpu_weight: CAPPED_WEIGHT,
        }
    });
    let incident = Incident { at: sample.at.clone(), load1: sample.load1, load_per_core: round(load_per_core), psi60: sample.psi60, trees, capped: capped.clone() };
    let text = message(&incident);
    let mut recipients: Vec<String> = Vec::new();
    let named = incident.trees.iter().find(|tree| tree.session.is_some());
    let capped_tree = capped.as_ref().and_then(|cap| incident.trees.iter().find(|tree| tree.unit == cap.unit));
    for tree in [named, capped_tree].into_iter().flatten() {
        recipients.extend(tree.session.clone());
        recipients.extend(tree.parent.clone());
    }
    recipients.push(COORDINATOR.to_owned());
    let mut seen = std::collections::BTreeSet::new();
    recipients.retain(|name| seen.insert(name.clone()));
    let mut actions = vec![Action::Incident(incident)];
    if let Some(cap) = capped {
        state.caps.push(cap.clone());
        actions.push(Action::Cap(cap));
    }
    actions.extend(recipients.into_iter().map(|to| Action::Send { to, text: text.clone() }));
    actions
}

fn ranked(cores: &BTreeMap<String, f64>) -> Vec<(String, f64)> {
    let mut leaves: Vec<(String, f64)> = cores
        .iter()
        .filter(|(cgroup, _)| is_unit(cgroup) && !cores.keys().any(|other| other.starts_with(&format!("{cgroup}/"))))
        .map(|(cgroup, peak)| (cgroup.clone(), *peak))
        .collect();
    leaves.sort_by(|a, b| b.1.total_cmp(&a.1));
    leaves
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub(super) fn message(incident: &Incident) -> String {
    let mut text = format!("watchdog: host overload, load {} ({}/core), cpu pressure {}% over 60 s. Top process trees, fleet-owned first:", incident.load1, incident.load_per_core, incident.psi60);
    for tree in &incident.trees {
        let owner = tree.session.as_deref().map_or_else(|| format!("no fleet session ({})", tree.kind), |session| format!("session {session}"));
        text.push_str(&format!("\n- {owner}: {} cores in {}{}", tree.cores, tree.unit, tree.lead.as_deref().map_or_else(String::new, |lead| format!(", lead {lead}"))));
    }
    match &incident.capped {
        Some(cap) => text.push_str(&format!(
            "\nCapped {} to {}% of one core and cpu weight {} (runtime, reversible); it lifts by itself once load stays calm. Stop the fan-out (parallel test runs, forked CLIs) and use the machine-capacity lease.",
            cap.unit, cap.cpu_quota_percent, cap.cpu_weight
        )),
        None => text.push_str("\nNo unleased fleet session scope is over the offender floor, so nothing was capped."),
    }
    text
}
