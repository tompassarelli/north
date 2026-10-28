//! Claude-only launch placement for `north fleet launch`: the cloud credit first for code-only public
//! work, then the Claude account with the most headroom, run on the first machine in `[placement] order`
//! that serves every local need of the box and is below its resident cap, else here. Pace lines, weekly
//! projections and the controller's held or over never hold a launch; an account is out only by the
//! five-hour stop, the five-hour burn cap or a provider-exhausted week (`crate::pace::exhaustion`).
use super::super::storage::{self, load, rows};
use super::super::{sampling, target};
use super::{credit, facets, field, local, resident, signed_in, strings};
use crate::capacity::agent_load;
use crate::fleet::model::ProviderName;
use crate::pace::exhaustion;
use crate::fleet::roster::{Roster, Standing};
use serde_json::Value;
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item};

const PUBLIC_REPOS: [&str; 3] = ["north", "wisp", "smashcraft"];
const GAMING_NEEDS: [&str; 2] = ["native", "private"];
const LOCAL_NEEDS: [&str; 12] = [
    "warcraft", "wc3", "client", "native", "sign-ins", "private", "local-state", "tui", "fleet", "live", "desktop", "laptop",
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Verdict {
    pub account: String,
    pub available: bool,
    pub headroom: f64,
    pub line: String,
    pub owners: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    Cloud(String),
    Account(String),
    Held(String),
    Unserved(String),
}

/// A `[hosts.NAME]` with an `ssh` destination: the local needs it serves beyond code, each `NEED` for
/// every repository or `NEED:REPO` for one, its resident cap, its live resident CLIs (None when
/// unreachable) and the Claude labels signed in there.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Machine {
    pub name: String,
    pub ssh: String,
    pub serves: Vec<String>,
    pub cap: i64,
    pub live: Option<i64>,
    pub accounts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Decision {
    pub target: Target,
    pub code_only: bool,
    pub surface: String,
    pub verdicts: Vec<Verdict>,
    pub machine: Option<Machine>,
    pub here: String,
    pub here_lacks: Option<(String, String)>,
    pub machine_lines: Vec<String>,
}

impl Decision {
    pub(crate) fn best_account(&self) -> Option<&Verdict> {
        self.best_where(|_| true)
    }

    /// The roomiest available account that runs here, the one a box kept here launches on.
    pub(crate) fn best_here(&self) -> Option<&Verdict> {
        let here: Vec<&str> = self.verdicts.iter().filter(|verdict| verdict.owners.is_empty()).map(|verdict| verdict.account.trim_start_matches("claude:")).collect();
        self.best_where(|label| here.contains(&label))
    }

    fn best_where(&self, allowed: impl Fn(&str) -> bool) -> Option<&Verdict> {
        roomiest(&self.verdicts, allowed)
    }

    fn sign_in_on_machine(&mut self) {
        let (Some(machine), Target::Account(account)) = (&self.machine, &self.target) else { return };
        let (name, account, accounts) = (machine.name.clone(), account.clone(), machine.accounts.clone());
        if accounts.iter().any(|label| label == account.trim_start_matches("claude:")) {
            return;
        }
        match self.best_where(|label| accounts.iter().any(|there| there == label)).map(|v| v.account.clone()) {
            Some(other) => {
                self.machine_lines.push(format!("{name} has no signed-in {account}; {other} there"));
                self.target = Target::Account(other);
            }
            None => self.stay_here(Some(format!("{name} has no signed-in {account}, so here"))),
        }
    }

    fn keep_here_on_local_account(&mut self) {
        let (None, Target::Account(account)) = (&self.machine, &self.target) else { return };
        let Some(owners) = self.verdict(account).map(|verdict| verdict.owners.join(" and ")).filter(|owners| !owners.is_empty()) else { return };
        let account = account.clone();
        let local: Vec<String> = self.verdicts.iter().filter(|verdict| verdict.owners.is_empty()).map(|verdict| verdict.account.trim_start_matches("claude:").to_owned()).collect();
        match self.best_where(|label| local.iter().any(|here| here == label)).map(|verdict| verdict.account.clone()) {
            Some(other) => {
                self.machine_lines.push(format!("{account} runs only on {owners}; {other} here"));
                self.target = Target::Account(other);
            }
            None => self.target = Target::Held(format!("{account} runs only on {owners}, and no account here is available")),
        }
    }

    pub(crate) fn run_on(&mut self, machine: Machine) {
        let (name, accounts) = (machine.name.clone(), machine.accounts.clone());
        self.machine_lines.push(format!("{name} named by --cwd"));
        self.machine = Some(machine);
        if matches!(self.target, Target::Held(_)) {
            return;
        }
        self.target = match self.best_where(|label| accounts.iter().any(|there| there == label)) {
            Some(best) => Target::Account(best.account.clone()),
            None => Target::Held(format!("no available Claude account is signed in on {name}")),
        };
    }

    pub(crate) fn verdict(&self, account: &str) -> Option<&Verdict> {
        self.verdicts.iter().find(|v| v.account == account)
    }

    pub(crate) fn stay_here(&mut self, line: Option<String>) {
        self.machine = None;
        self.machine_lines.extend(line);
        if let Some((need, why)) = self.here_lacks.clone() {
            self.machine_lines.push(format!("{} skipped: needs {need}", self.here));
            self.target = Target::Unserved(why);
        }
    }
}

fn roomiest(verdicts: &[Verdict], allowed: impl Fn(&str) -> bool) -> Option<&Verdict> {
    verdicts
        .iter()
        .filter(|v| v.available && allowed(v.account.trim_start_matches("claude:")))
        .max_by(|a, b| a.headroom.total_cmp(&b.headroom).then(b.account.cmp(&a.account)))
}

/// The label of the Claude account that runs here with the most headroom in the gate pace-control
/// last wrote, None when every one is out. The measured burn does not count: a probe of two short
/// replies cannot drain a window, so only the five-hour stop and an exhausted week rule an account out.
pub(crate) fn roomiest_here(gate: &Value, roster: &Roster) -> Option<String> {
    let here: Vec<Verdict> = standing_verdicts(gate, roster, false).into_iter().filter(|verdict| verdict.owners.is_empty()).collect();
    roomiest(&here, |_| true).map(|best| best.account.trim_start_matches("claude:").to_owned())
}

pub(crate) fn live_gate() -> Value {
    load(&storage::directory().join("usage-gate.json"))
}

fn window_refusal(window: &Value, burn_counts: bool) -> Option<String> {
    let name = window["window"].as_str().unwrap_or("window");
    let burn = window["burn_pct_h"].as_f64().filter(|_| burn_counts);
    exhaustion::refusal(name, window["used_pct"].as_f64()?, burn)
}

fn headroom(windows: &[Value]) -> f64 {
    windows
        .iter()
        .filter_map(|w| exhaustion::headroom(w["window"].as_str()?, w["used_pct"].as_f64()?))
        .min_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn usage(windows: &[Value]) -> String {
    windows
        .iter()
        .filter_map(|w| Some(format!("{} {:.0}%", w["window"].as_str()?, w["used_pct"].as_f64()?)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn verdict(account: &str, control: &Value) -> Verdict {
    judged(account, control, true)
}

fn judged(account: &str, control: &Value, burn_counts: bool) -> Verdict {
    let windows = rows(&control["windows"]);
    if windows.iter().all(|w| w["used_pct"].as_f64().is_none()) {
        return unavailable(account, "no live quota sample".into());
    }
    if let Some(reason) = windows.iter().find_map(|window| window_refusal(window, burn_counts)) {
        return unavailable(account, reason);
    }
    let room = headroom(windows);
    Verdict {
        account: account.into(),
        available: true,
        headroom: room,
        line: format!("{account} available: {} ({room:.0}% headroom)", usage(windows)),
        owners: Vec::new(),
    }
}

pub(crate) fn repository(text: &str) -> String {
    let declared = field(text, "Repository");
    let source = if declared.is_empty() {
        field(text, "Item").split('#').next().unwrap_or_default()
    } else {
        declared.split_whitespace().next().unwrap_or_default()
    };
    let last = source.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    last.trim_end_matches(".git").trim().to_lowercase()
}

pub(crate) fn words(text: &str, key: &str) -> Vec<String> {
    field(text, key)
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',' || c == '|' || c == '/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
/// The needs that keep a box off code-only targets: local `Needs:` and `Surface:` words, and native
/// verification.
fn local_needs(text: &str) -> Vec<String> {
    let (_, native, needs) = facets(text);
    let mut local: Vec<String> = words(text, "Surface")
        .into_iter()
        .chain(needs)
        .chain(words(text, "Needs"))
        .filter(|w| LOCAL_NEEDS.contains(&w.as_str()))
        .chain(native.then(|| "native".to_owned()))
        .collect();
    local.sort();
    local.dedup();
    local
}

/// A box is code-only only when its brief declares a code `Surface:`, needs nothing local and names
/// a public repository; anything undeclared stays local.
pub(crate) fn surface(text: &str, cfg: &DocumentMut) -> (bool, String) {
    if words(text, "Surface").is_empty() {
        return (false, "no Surface: declared, so local".into());
    }
    if let Some(need) = local_needs(text).first() {
        return (false, format!("needs {need}, so local"));
    }
    let repo = repository(text);
    let rules = cfg.get("placement");
    let private = strings(rules.and_then(|p| p.get("private_repos")));
    let mut public = strings(rules.and_then(|p| p.get("public_repos")));
    if public.is_empty() {
        public = PUBLIC_REPOS.iter().map(|s| (*s).to_owned()).collect();
    }
    if repo.is_empty() || private.contains(&repo) || !public.contains(&repo) {
        let name = if repo.is_empty() { "no repository" } else { repo.as_str() };
        return (false, format!("{name} is not a public repository, so local"));
    }
    (true, format!("code-only box on public {repo}"))
}

/// Machines in `[placement] order`, then any other `[hosts.NAME]` naming an `ssh` destination, unprobed.
pub(crate) fn machines(cfg: &DocumentMut) -> Vec<Machine> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    let order = strings(cfg.get("placement").and_then(|p| p.get("order")));
    let rest: Vec<String> = hosts.iter().map(|(name, _)| name.to_owned()).filter(|name| !order.contains(name)).collect();
    order
        .into_iter()
        .chain(rest)
        .filter_map(|name| {
            let host = hosts.get(&name)?;
            let ssh = host.get("ssh")?.as_str()?.to_owned();
            let cap = host.get("resident_cap").and_then(Item::as_integer).unwrap_or(0);
            Some(Machine { serves: strings(host.get("serves")), name, ssh, cap, live: None, accounts: Vec::new() })
        })
        .collect()
}

pub(crate) fn serving(cfg: &DocumentMut, needs: &str, here: &str) -> Option<String> {
    let needs: Vec<(&str, &str)> = needs.split(',').map(|need| need.split_once(':').unwrap_or((need, ""))).collect();
    machines(cfg)
        .into_iter()
        .filter(|machine| machine.name != here)
        .find(|machine| needs.iter().all(|(need, repo)| covers(&machine.serves, need, repo)))
        .map(|machine| machine.name)
}

pub(crate) fn warcraft_clients(cfg: &DocumentMut, here: &str) -> Option<u32> {
    cfg.get("hosts")?.get(here)?.get("warcraft_clients")?.as_integer()?.try_into().ok()
}

pub(crate) fn bar1_margin_mib(cfg: &DocumentMut, here: &str) -> Option<u32> {
    cfg.get("hosts")?.get(here)?.get("bar1_margin_mib")?.as_integer()?.try_into().ok()
}

pub(crate) fn capacity_need(text: &str) -> Option<String> {
    let repo = repository(text);
    let needs: Vec<String> = local_needs(text).into_iter().map(|need| if repo.is_empty() { need } else { format!("{need}:{repo}") }).collect();
    (!needs.is_empty()).then(|| needs.join(","))
}

/// The first local need of the box that `[hosts.HOST] serves` does not cover.
pub(crate) fn lacks(cfg: &DocumentMut, host: &str, text: &str) -> Option<String> {
    let serves = strings(cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("serves")));
    let repo = repository(text);
    local_needs(text).into_iter().find(|need| !covers(&serves, need, &repo))
}

fn covers(serves: &[String], need: &str, repo: &str) -> bool {
    serves.iter().any(|served| served == need || served.split_once(':') == Some((need, repo)))
}

/// The first machine that serves every local need of the box and is below its cap, with one line per
/// machine, and the need here does not serve with why no host serves it.
fn machine_for(text: &str, machines: &[Machine], here_serves: &[String], here_gaming: bool) -> (Option<Machine>, Vec<String>, Option<(String, String)>) {
    let declared = !words(text, "Surface").is_empty();
    let needs = if declared || !here_gaming { local_needs(text) } else { GAMING_NEEDS.iter().map(|need| (*need).to_owned()).collect() };
    let repo = repository(text);
    let missing = |serves: &[String]| needs.iter().find(|need| !covers(serves, need, &repo));
    let mut chosen = None;
    let lines = machines
        .iter()
        .map(|machine| {
            let name = &machine.name;
            if !declared && !here_gaming {
                return format!("{name} skipped: no Surface: declared");
            }
            if let Some(need) = missing(&machine.serves) {
                return format!("{name} skipped: needs {need}");
            }
            match machine.live {
                None => format!("{name} unreachable over ssh {}", machine.ssh),
                Some(live) if live >= machine.cap => format!("{name} full: {live}/{} resident CLIs", machine.cap),
                Some(live) => {
                    chosen.get_or_insert_with(|| machine.clone());
                    format!("{name} available: {live}/{} resident CLIs", machine.cap)
                }
            }
        })
        .collect();
    let here_lacks = missing(here_serves).map(|need| (need.clone(), format!("no host serves {need} for {repo}")));
    (chosen, lines, here_lacks)
}

fn claude_controls(gate: &Value) -> Vec<(String, &Value)> {
    gate["claude"]["controls"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| name.starts_with("claude:"))
        .map(|(name, control)| (name.clone(), control))
        .collect()
}

pub(crate) fn verdicts(gate: &Value, roster: &Roster) -> Vec<Verdict> {
    standing_verdicts(gate, roster, true)
}

fn standing_verdicts(gate: &Value, roster: &Roster, burn_counts: bool) -> Vec<Verdict> {
    let claude = ProviderName::new("claude");
    claude_controls(gate)
        .into_iter()
        .map(|(name, control)| match roster.standing(&claude, name.trim_start_matches("claude:")) {
            Standing::Listed(_) => judged(&name, control, burn_counts),
            Standing::Duplicate(duplicate) => unavailable(&name, duplicate.to_string()),
            Standing::Foreign(foreign) => Verdict { owners: foreign.owners.clone(), ..judged(&name, control, burn_counts) },
            Standing::Unknown => unavailable(&name, "no fleet profile".into()),
        })
        .collect()
}

fn unavailable(account: &str, reason: String) -> Verdict {
    Verdict { account: account.into(), available: false, headroom: 0.0, line: format!("{account} unavailable: {reason}"), owners: Vec::new() }
}

pub(crate) fn decide(text: &str, cfg: &DocumentMut, verdicts: Vec<Verdict>, credit: (bool, String), machines: &[Machine], here: &str, here_gaming: bool) -> Decision {
    let (code_only, surface) = surface(text, cfg);
    let here_serves = strings(cfg.get("hosts").and_then(|hosts| hosts.get(here)).and_then(|host| host.get("serves")));
    let (machine, machine_lines, here_lacks) = machine_for(text, machines, &here_serves, here_gaming);
    let mut decision = Decision {
        target: Target::Held(String::new()),
        code_only,
        surface,
        verdicts,
        machine: None,
        here: here.into(),
        here_lacks,
        machine_lines,
    };
    decision.target = if code_only && credit.0 {
        Target::Cloud(credit.1)
    } else if let Some(best) = decision.best_account().map(|best| best.account.clone()) {
        Target::Account(best)
    } else if decision.verdicts.is_empty() {
        Target::Held("no live Claude quota in usage-gate.json; run pace-control".into())
    } else {
        Target::Held("every Claude account exhausted or over its burn cap".into())
    };
    match machine {
        Some(machine) if matches!(decision.target, Target::Account(_)) => decision.machine = Some(machine),
        Some(_) => {}
        None => decision.stay_here(None),
    }
    decision.sign_in_on_machine();
    decision.keep_here_on_local_account();
    decision
}

/// `target anthropic-cloud (credit; surface)`, `target HOST claude:c (surface)`, `target held: why` or
/// `target unserved: why`.
pub(crate) fn summary(decision: &Decision) -> String {
    match (&decision.target, &decision.machine) {
        (Target::Cloud(credit), _) => format!("target anthropic-cloud ({credit}; {})", decision.surface),
        (Target::Account(account), Some(machine)) => format!("target {} {account} ({})", machine.name, decision.surface),
        (Target::Account(account), None) => format!("target {} {account} ({})", decision.here, decision.surface),
        (Target::Held(why), _) => format!("target held: {why}"),
        (Target::Unserved(why), _) => format!("target unserved: {why}"),
    }
}

/// The live decision: South's orchestration.toml, the gate pace-control last wrote, the credit and
/// each machine's load, given the fleet sessions this machine runs there by machine name.
pub(crate) fn live(text: &str, fleet_load: &BTreeMap<String, i64>, roster: &Roster, named: Option<&str>) -> Result<Decision, String> {
    let cfg = sampling::config()?;
    let at = std::env::var("PLACEMENT_NOW")
        .ok()
        .and_then(|s| target::seconds(&s))
        .unwrap_or_else(storage::now);
    let gate = live_gate();
    let machines = probed(&cfg, fleet_load);
    let mut decision = decide(text, &cfg, verdicts(&gate, roster), credit(&cfg, at), &machines, &local(), here_in_game_reserve());
    if let Some(name) = named {
        let machine = machines.into_iter().find(|machine| machine.name == name).ok_or_else(|| format!("no [hosts.{name}] with an ssh destination"))?;
        decision.run_on(machine);
    }
    Ok(decision)
}

pub(crate) fn here_in_game_reserve() -> bool {
    agent_load::game_reserve_on(&agent_load::Host::from_env())
}

/// Each machine's live load: the resident CLIs it reports over ssh, which never include fleet
/// sessions hosted elsewhere, plus the fleet sessions given by machine name.
pub(crate) fn probed(cfg: &DocumentMut, fleet_load: &BTreeMap<String, i64>) -> Vec<Machine> {
    machines(cfg)
        .into_iter()
        .map(|machine| {
            let hosted = fleet_load.get(&machine.name).copied().unwrap_or(0);
            Machine { live: resident(&machine.name, &machine.ssh).map(|live| live + hosted), accounts: signed_in(&machine.name, &machine.ssh), ..machine }
        })
        .collect()
}
