//! Claude-only launch placement for `north fleet launch`: Anthropic cloud on the Claude account with the most headroom for code-only public
//! work, else the Claude account the shared rank puts first (`room::Rank`), run on the first machine in `[placement] order`
//! that serves every local need of the box and is below its resident cap, else here. Pace lines, weekly
//! projections and the controller's held or over never hold a launch; an account is out only by the
//! five-hour stop, the five-hour burn cap or a provider-exhausted week (`crate::pace::exhaustion`).
use super::super::storage::{self, load, rows};
use super::super::{sampling, target};
pub use super::repository::{anchored, repository};
use super::needs::{LOCAL_NEEDS, words};
use super::reach::Reach;
use super::room::{Basis, Rank};
use super::{credit, facets, here_saturation, local, strings};
use crate::capacity::{agent_load, laptop_auto};
use crate::capacity::saturation::Saturation;
use crate::fleet::model::ProviderName;
use crate::pace::exhaustion;
use crate::fleet::roster::{Roster, Standing};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;
use toml_edit::{DocumentMut, Item};

const PUBLIC_REPOS: [&str; 3] = ["north", "wisp", "makgora"];
const GAMING_NEEDS: [&str; 2] = ["native", "private"];

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub account: String,
    pub available: bool,
    /// Out by the five-hour stop, the burn cap or an exhausted week, which no `--pin` overrides.
    pub exhausted: bool,
    pub headroom: f64,
    /// Zero until `verdicts` ranks it from the account's room, plan and spend target.
    pub rank: Rank,
    pub line: String,
    pub owners: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudLane {
    pub account: String,
    pub host: String,
    pub credit: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Cloud(CloudLane),
    Account(String),
    Held(String),
    Unserved(String),
}

/// A `[hosts.NAME]` with an `ssh` destination: the local needs it serves beyond code, each `NEED` for
/// every repository or `NEED:REPO` for one, its resident cap, its live resident CLIs (None when
/// unreachable) and the Claude labels signed in there. `pushes` names the repositories a landing from
/// there can push, None when the host declares no list and so pushes any.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub name: String,
    pub ssh: String,
    pub serves: Vec<String>,
    pub pushes: Option<Vec<String>>,
    /// The `[hosts]` names a session there can reach over ssh, None when the host declares no list and so reaches any.
    pub reaches: Option<Vec<String>>,
    pub cap: i64,
    pub live: Option<i64>,
    pub accounts: Vec<String>,
    pub saturation: Option<Saturation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub target: Target,
    pub code_only: bool,
    pub surface: String,
    pub verdicts: Vec<Verdict>,
    pub machine: Option<Machine>,
    pub here: String,
    pub here_lacks: Option<(String, String)>,
    pub machine_lines: Vec<String>,
    pub saturated: Option<String>,
    pub here_saturated: Option<String>,
    pub here_on_battery: Option<String>,
    /// Machines that serve the box and were skipped as saturated, each with why.
    pub crowded: Vec<String>,
    /// Why the launching host cannot take what crowded machines turned away: on battery or under a playtest lease.
    pub here_closed: Option<String>,
}

impl Decision {
    fn cloud_lane(&self, credit: String) -> Option<CloudLane> {
        let best = self.best_account()?;
        let host = best.owners.first().cloned().unwrap_or_else(|| self.here.clone());
        Some(CloudLane { account: best.account.clone(), host, credit })
    }

    pub fn best_account(&self) -> Option<&Verdict> {
        self.best_where(|_| true)
    }

    /// The best-ranked available account that runs here, the one a box kept here launches on.
    pub fn best_here(&self) -> Option<&Verdict> {
        let here: Vec<&str> = self.verdicts.iter().filter(|verdict| verdict.owners.is_empty()).map(|verdict| verdict.account.trim_start_matches("claude:")).collect();
        self.best_where(|label| here.contains(&label))
    }

    fn best_where(&self, allowed: impl Fn(&str) -> bool) -> Option<&Verdict> {
        best(&self.verdicts, allowed, |a, b| a.rank.order(&b.rank))
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

    pub fn run_on(&mut self, machine: Machine) {
        let (name, accounts) = (machine.name.clone(), machine.accounts.clone());
        self.machine_lines.push(format!("{name} named by --cwd"));
        self.saturated = machine.saturation_refusal();
        self.machine_lines.extend(self.saturated.clone());
        self.machine = Some(machine);
        if matches!(self.target, Target::Held(_)) {
            return;
        }
        self.target = match self.best_where(|label| accounts.iter().any(|there| there == label)) {
            Some(best) => Target::Account(best.account.clone()),
            None => Target::Held(format!("no available Claude account is signed in on {name}")),
        };
    }

    pub fn verdict(&self, account: &str) -> Option<&Verdict> {
        self.verdicts.iter().find(|v| v.account == account)
    }

    pub fn stay_here(&mut self, line: Option<String>) {
        self.machine = None;
        self.machine_lines.extend(line);
        if let Some((need, why)) = self.here_lacks.clone() {
            self.machine_lines.push(format!("{} skipped: needs {need}", self.here));
            self.target = Target::Unserved(why);
        }
    }
}

fn best(verdicts: &[Verdict], allowed: impl Fn(&str) -> bool, order: impl Fn(&Verdict, &Verdict) -> Ordering) -> Option<&Verdict> {
    verdicts.iter().filter(|v| v.available && allowed(v.account.trim_start_matches("claude:"))).max_by(|a, b| order(a, b).then(b.account.cmp(&a.account)))
}

/// The label of the Claude account that runs here with the most headroom in the gate pace-control
/// last wrote, None when every one is out. The measured burn does not count: a probe of two short
/// replies cannot drain a window, so only the five-hour stop and an exhausted week rule an account out.
pub fn roomiest_here(gate: &Value, roster: &Roster) -> Option<String> {
    let here: Vec<Verdict> = standing_verdicts(gate, roster, false).into_iter().filter(|verdict| verdict.owners.is_empty()).collect();
    best(&here, |_| true, |a, b| a.headroom.total_cmp(&b.headroom)).map(|best| best.account.trim_start_matches("claude:").to_owned())
}

pub fn live_gate() -> Value {
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

pub fn verdict(account: &str, control: &Value) -> Verdict {
    judged(account, control, true)
}

fn judged(account: &str, control: &Value, burn_counts: bool) -> Verdict {
    let windows = rows(&control["windows"]);
    if windows.iter().all(|w| w["used_pct"].as_f64().is_none()) {
        return unavailable(account, "no live quota sample".into());
    }
    if let Some(reason) = windows.iter().find_map(|window| window_refusal(window, burn_counts)) {
        return Verdict { exhausted: true, ..unavailable(account, reason) };
    }
    let room = headroom(windows);
    Verdict {
        account: account.into(),
        available: true,
        exhausted: false,
        headroom: room,
        rank: Rank::default(),
        line: format!("{account} available: {} ({room:.0}% headroom)", usage(windows)),
        owners: Vec::new(),
    }
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
pub fn surface(text: &str, cfg: &DocumentMut) -> (bool, String) {
    if words(text, "Surface").is_empty() {
        return (false, "no Surface: declared, not cloud".into());
    }
    if let Some(need) = local_needs(text).first() {
        return (false, format!("needs {need}, not cloud"));
    }
    if let Some(host) = Reach::of(text, cfg, "").named().first() {
        return (false, format!("needs {host}, not cloud"));
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
        return (false, format!("{name} is not a public repository, not cloud"));
    }
    (true, format!("code-only box on public {repo}"))
}

pub fn ssh_destination(value: &str) -> Option<&str> {
    let plain = value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._@-".contains(&byte));
    (plain && !value.is_empty() && !value.starts_with('-')).then_some(value)
}

/// Machines in `[placement] order`, then any other `[hosts.NAME]` naming an `ssh` destination, unprobed.
pub fn machines(cfg: &DocumentMut) -> Vec<Machine> {
    let Some(hosts) = cfg.get("hosts").and_then(Item::as_table_like) else { return Vec::new() };
    let order = strings(cfg.get("placement").and_then(|p| p.get("order")));
    let rest: Vec<String> = hosts.iter().map(|(name, _)| name.to_owned()).filter(|name| !order.contains(name)).collect();
    order
        .into_iter()
        .chain(rest)
        .filter_map(|name| {
            let host = hosts.get(&name)?;
            let ssh = ssh_destination(host.get("ssh")?.as_str()?)?.to_owned();
            let cap = host.get("resident_cap").and_then(Item::as_integer).unwrap_or(0);
            Some(Machine { serves: strings(host.get("serves")), pushes: pushes(host), reaches: super::reach::reaches(host), name, ssh, cap, live: None, accounts: Vec::new(), saturation: None })
        })
        .collect()
}

/// The `[hosts.NAME]` machines marked `pulls_spool = true`: each far machine whose CLIs this fleet runs whether or not a session is placed there now.
pub fn spool_pulls(cfg: &DocumentMut, here: &str) -> Vec<(String, String)> {
    machines(cfg).into_iter().filter(|machine| machine.name != here).filter(|machine| spool_pulled(cfg, &machine.name)).map(|machine| (machine.name, machine.ssh)).collect()
}

pub fn spool_pulled(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("pulls_spool")).and_then(Item::as_bool) == Some(true)
}

pub fn job_source(cfg: &DocumentMut, host: &str) -> bool {
    cfg.get("hosts").and_then(|hosts| hosts.get(host)).and_then(|host| host.get("job_source")).and_then(Item::as_bool) == Some(true)
}

pub fn serving(cfg: &DocumentMut, needs: &str, here: &str) -> Option<String> {
    let needs: Vec<(&str, &str)> = needs.split(',').map(|need| need.split_once(':').unwrap_or((need, ""))).collect();
    machines(cfg)
        .into_iter()
        .filter(|machine| machine.name != here)
        .find(|machine| needs.iter().all(|(need, repo)| covers(&machine.serves, need, repo)))
        .map(|machine| machine.name)
}

pub fn warcraft_clients(cfg: &DocumentMut, here: &str) -> Option<u32> {
    cfg.get("hosts")?.get(here)?.get("warcraft_clients")?.as_integer()?.try_into().ok()
}

pub fn bar1_margin_mib(cfg: &DocumentMut, here: &str) -> Option<u32> {
    cfg.get("hosts")?.get(here)?.get("bar1_margin_mib")?.as_integer()?.try_into().ok()
}

pub fn capacity_need(text: &str) -> Option<String> {
    let repo = repository(text);
    let needs: Vec<String> = local_needs(text).into_iter().map(|need| if repo.is_empty() { need } else { format!("{need}:{repo}") }).collect();
    (!needs.is_empty()).then(|| needs.join(","))
}

/// The first local need of the box that `[hosts.HOST] serves` does not cover.
pub fn lacks(cfg: &DocumentMut, host: &str, text: &str) -> Option<String> {
    let host = cfg.get("hosts").and_then(|hosts| hosts.get(host));
    let serves = strings(host.and_then(|host| host.get("serves")));
    let repo = repository(text);
    if !can_push(host.and_then(pushes).as_deref(), &repo) {
        return Some(format!("push to {repo}"));
    }
    local_needs(text).into_iter().find(|need| !covers(&serves, need, &repo))
}

/// The repositories a host declares it can push, None when it declares no list.
fn pushes(host: &Item) -> Option<Vec<String>> {
    host.get("pushes").map(|_| strings(host.get("pushes")))
}

fn can_push(pushes: Option<&[String]>, repo: &str) -> bool {
    repo.is_empty() || pushes.is_none_or(|pushes| pushes.iter().any(|pushed| pushed == repo))
}

fn covers(serves: &[String], need: &str, repo: &str) -> bool {
    serves.iter().any(|served| served == need || served.split_once(':') == Some((need, repo)))
}

struct Choice {
    chosen: Option<Machine>,
    lines: Vec<String>,
    here_lacks: Option<(String, String)>,
    crowded: Vec<String>,
}

/// The first machine that serves every local need of the box and is below its cap, with one line per
/// machine, the need here does not serve with why no host serves it, and the machines skipped as saturated.
fn machine_for(text: &str, machines: &[Machine], here_serves: &[String], here_gaming: bool, reach: &Reach) -> Choice {
    let declared = !words(text, "Surface").is_empty();
    let needs = if declared || !here_gaming { local_needs(text) } else { GAMING_NEEDS.iter().map(|need| (*need).to_owned()).collect() };
    let repo = repository(text);
    let missing = |serves: &[String]| needs.iter().find(|need| !covers(serves, need, &repo));
    let mut chosen = None;
    let mut crowded = Vec::new();
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
            if !can_push(machine.pushes.as_deref(), &repo) {
                return format!("{name} skipped: cannot push {repo}");
            }
            if let Some(host) = reach.unreached(name, machine.reaches.as_deref()) {
                return format!("{name} skipped: cannot reach {host}");
            }
            if let Some(why) = machine.saturation_refusal() {
                crowded.push(why.clone());
                return why;
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
    let here_lacks = missing(here_serves).map(|need| (need.clone(), format!("no host serves {need} for {repo}"))).or_else(|| reach.unreached_here().map(|host| (host.to_owned(), format!("no host reaches {host}"))));
    Choice { chosen, lines, here_lacks, crowded }
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

pub fn verdicts(gate: &Value, roster: &Roster, basis: &Basis) -> Vec<Verdict> {
    standing_verdicts(gate, roster, true)
        .into_iter()
        .map(|verdict| Verdict { rank: basis.rank(verdict.available, &verdict.account, &gate["claude"]["controls"][&verdict.account]), ..verdict })
        .collect()
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
    Verdict { account: account.into(), available: false, exhausted: false, headroom: 0.0, rank: Rank::default(), line: format!("{account} unavailable: {reason}"), owners: Vec::new() }
}

pub fn decide(text: &str, cfg: &DocumentMut, verdicts: Vec<Verdict>, credit: (bool, String), machines: &[Machine], here: &str, here_gaming: bool) -> Decision {
    let (code_only, surface) = surface(text, cfg);
    let here_serves = strings(cfg.get("hosts").and_then(|hosts| hosts.get(here)).and_then(|host| host.get("serves")));
    let reach = Reach::of(text, cfg, here);
    let Choice { chosen: machine, lines: machine_lines, here_lacks, crowded } = machine_for(text, machines, &here_serves, here_gaming, &reach);
    let mut decision = Decision {
        target: Target::Held(String::new()),
        code_only,
        surface,
        verdicts,
        machine: None,
        here: here.into(),
        here_lacks,
        machine_lines,
        saturated: None,
        here_saturated: None,
        here_on_battery: None,
        crowded,
        here_closed: None,
    };
    decision.target = if let Some(lane) = code_only.then(|| decision.cloud_lane(credit.1)).flatten() {
        Target::Cloud(lane)
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

/// `target anthropic-cloud claude:d on HOST (credit; surface)`, `target HOST claude:c (surface)`, `target held: why` or
/// `target unserved: why`.
pub fn summary(decision: &Decision) -> String {
    match (&decision.target, &decision.machine) {
        (Target::Cloud(lane), _) => format!("target anthropic-cloud {} on {} ({}; {})", lane.account, lane.host, lane.credit, decision.surface),
        (Target::Account(account), Some(machine)) => format!("target {} {account} ({})", machine.name, decision.surface),
        (Target::Account(account), None) => format!("target {} {account} ({})", decision.here, decision.surface),
        (Target::Held(why), _) => format!("target held: {why}"),
        (Target::Unserved(why), _) => format!("target unserved: {why}"),
    }
}

/// The live decision: South's orchestration.toml, the gate pace-control last wrote, the credit and
/// each machine's load, given the fleet sessions this machine runs there by machine name.
pub fn live(text: &str, fleet_load: &BTreeMap<String, i64>, roster: &Roster, named: Option<&str>) -> Result<Decision, String> {
    let cfg = sampling::config()?;
    let at = std::env::var("PLACEMENT_NOW")
        .ok()
        .and_then(|s| target::seconds(&s))
        .unwrap_or_else(storage::now);
    let gate = live_gate();
    let machines = probed(&cfg, fleet_load);
    let here = local();
    let here_on_battery = on_battery(&here, &laptop_auto::power_supply(), &laptop_auto::discharging_since(&agent_load::Host::from_env()), crate::capacity::mode::now(), &machines);
    let mut decision = decide(text, &cfg, verdicts(&gate, roster, &Basis::new(&cfg, load(&storage::directory().join("pace-control.json")), at)), credit(&cfg, at), &machines, &here, here_in_game_reserve());
    decision.here_on_battery = here_on_battery;
    decision.here_closed = here_closed(&here, &laptop_auto::power_supply(), &laptop_auto::discharging_since(&agent_load::Host::from_env()), &crate::capacity::mode::read(&crate::capacity::mode::state_dir()), crate::capacity::mode::now());
    if let Some(name) = named {
        let machine = machines.into_iter().find(|machine| machine.name == name).ok_or_else(|| format!("no [hosts.{name}] with an ssh destination"))?;
        decision.run_on(machine);
    }
    decision.here_saturated = here_saturation();
    decision.hold_here();
    Ok(decision)
}

pub fn on_battery(here: &str, power_supply: &Path, since: &Path, now: u64, machines: &[Machine]) -> Option<String> {
    let percent = laptop_auto::settled_battery(power_supply, since, now)?;
    let elsewhere: Vec<&str> = machines.iter().map(|machine| machine.name.as_str()).filter(|name| *name != here).collect();
    let instead = if elsewhere.is_empty() { "in anthropic-cloud".to_owned() } else { format!("on {} or in anthropic-cloud", elsewhere.join(" or ")) };
    Some(format!(
        "{here} is a laptop on battery (discharging at {percent}%), so --local would drain it; launch the box {instead} without --local, or keep it here with --pin \"<reason>\""
    ))
}

/// Why `here` takes no work that crowded machines turned away: a playtest lease is on, read from the capacity mode state and not inferred from an open game, or the laptop is on battery.
pub fn here_closed(here: &str, power_supply: &Path, since: &Path, state: &crate::capacity::mode::State, now: u64) -> Option<String> {
    if let Some(hold) = state.hold.as_ref().filter(|hold| hold.until > now) {
        let until = crate::foundation::clock::Timestamp::from_unix_seconds(i64::try_from(hold.until).unwrap_or(i64::MAX)).local_twelve_hour();
        return Some(format!("{here} is under a playtest lease until {until}"));
    }
    let percent = laptop_auto::settled_battery(power_supply, since, now)?;
    Some(format!("{here} is on battery (discharging at {percent}%)"))
}

pub fn here_in_game_reserve() -> bool {
    agent_load::gaming_declared(&agent_load::Host::from_env())
}

/// Each machine's live load: the resident CLIs it reports over ssh, which never include fleet
/// sessions hosted elsewhere, plus the fleet sessions given by machine name.
pub fn probed(cfg: &DocumentMut, fleet_load: &BTreeMap<String, i64>) -> Vec<Machine> {
    super::hosts::probed_machines(machines(cfg), fleet_load)
}
