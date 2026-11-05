use std::collections::BTreeSet;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::sampling::round2;

#[derive(Debug, Deserialize)]
pub(super) struct Expected {
    system: Vec<String>,
    user: Vec<String>,
    sockets: Vec<String>,
    cores: f64,
    #[serde(default)]
    budgets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Unit {
    pub(super) scope: String,
    pub(super) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cores: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cs_per_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) agent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Observed {
    pub(super) seconds: f64,
    pub(super) units: Vec<Unit>,
    pub(super) sockets: Vec<String>,
}

pub(super) struct Verdict {
    ok: bool,
    extra_units: Vec<Unit>,
    extra_sockets: Vec<String>,
    over_budget: Vec<(Unit, f64)>,
    agents: Vec<Unit>,
    cores: f64,
    cs_per_sec: f64,
}

impl Verdict {
    pub(super) fn ok(&self) -> bool {
        self.ok
    }

    pub(super) fn lines(&self) -> Vec<String> {
        let measured = |unit: &Unit| unit.cores.map(|cores| format!(" ({cores} cores, {} cs/s)", unit.cs_per_sec.unwrap_or(0.0))).unwrap_or_default();
        let mut out: Vec<String> = self.extra_units.iter().map(|unit| format!("extra {} unit: {}{}", unit.scope, unit.name, measured(unit))).collect();
        out.extend(self.extra_sockets.iter().map(|socket| format!("extra socket: {socket}")));
        out.extend(self.over_budget.iter().map(|(unit, budget)| format!("over budget: {} {} {} cores > {budget} ({} cs/s)", unit.scope, unit.name, unit.cores.unwrap_or(0.0), unit.cs_per_sec.unwrap_or(0.0))));
        out.extend(self.agents.iter().map(|unit| format!("agent: {} {} ({}{})", unit.scope, unit.name, unit.agent.as_deref().unwrap_or_default(), unit.cores.map(|cores| format!(", {cores} cores")).unwrap_or_default())));
        out.push(format!(
            "rest-check: {}; {} extra units, {} extra sockets, {} over budget, {} agents; non-agent {} cores, {} cs/s",
            if self.ok { "at rest" } else { "not at rest" },
            self.extra_units.len(),
            self.extra_sockets.len(),
            self.over_budget.len(),
            self.agents.len(),
            self.cores,
            self.cs_per_sec
        ));
        out
    }
}

fn glob(pattern: &str) -> Regex {
    Regex::new(&format!("^{}$", regex::escape(pattern).replace("\\*", ".*"))).expect("an escaped glob is a regex")
}

pub(super) fn check(expected: &Expected, observed: &Observed) -> Verdict {
    let system: Vec<Regex> = expected.system.iter().map(|pattern| glob(pattern)).collect();
    let user: Vec<Regex> = expected.user.iter().map(|pattern| glob(pattern)).collect();
    let known = |unit: &Unit| {
        let allowed = if unit.scope == "system" { &system } else { &user };
        let timer = unit.name.strip_suffix(".service").map(|stem| format!("{stem}.timer"));
        allowed.iter().any(|pattern| pattern.is_match(&unit.name) || timer.as_ref().is_some_and(|timer| pattern.is_match(timer)))
    };
    let (agents, rest): (Vec<Unit>, Vec<Unit>) = observed.units.iter().cloned().partition(|unit| unit.agent.is_some());
    let extra_units: Vec<Unit> = rest.iter().filter(|unit| !known(unit)).cloned().collect();
    let sockets: Vec<Regex> = expected.sockets.iter().map(|pattern| glob(pattern)).collect();
    let extra_sockets: Vec<String> = observed.sockets.iter().collect::<BTreeSet<_>>().into_iter().filter(|socket| !sockets.iter().any(|pattern| pattern.is_match(socket))).cloned().collect();
    let budget = |name: &str| {
        expected.budgets.iter().filter_map(|entry| entry.split_once(char::is_whitespace)).find(|(unit, _)| *unit == name).and_then(|(_, cores)| cores.trim().parse().ok()).unwrap_or(expected.cores)
    };
    let over_budget: Vec<(Unit, f64)> = rest.iter().filter(|unit| known(unit)).filter_map(|unit| unit.cores.filter(|cores| *cores > budget(&unit.name)).map(|_| (unit.clone(), budget(&unit.name)))).collect();
    let cs_per_sec = round2(rest.iter().map(|unit| unit.cs_per_sec.unwrap_or(0.0)).sum());
    let cores = round2(rest.iter().map(|unit| unit.cores.unwrap_or(0.0)).sum());
    Verdict { ok: extra_units.is_empty() && extra_sockets.is_empty() && over_budget.is_empty(), extra_units, extra_sockets, over_budget, agents, cores, cs_per_sec }
}
