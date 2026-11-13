//! The placement gate in front of `north fleet launch` and `revive`: a Claude box starts only where
//! placement puts it, or with the override reason recorded on the session.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, LaunchRequest, Machine, Role};
use crate::fleet::registry::Fleet;
use crate::fleet::roster::Roster;
use crate::dispatch::{cloud_autoland, cloud_branch};
use crate::pacing::placement::{case, recent};
use crate::pacing::placement::gate::{self, CloudLane, Decision, Target};
use crate::platform::attribution::{self, Cause, Harness};
pub(super) use super::refusal::Refusal;
use crate::platform::decision_log;
use crate::spend::Starter;
use crate::usage::decide::world::ModelFamily;
use north_strategy_types::placement::{self as record, Case};
use north_strategy_types::Kind;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Overrides {
    pub local: Option<String>,
    pub pin: Option<String>,
    pub tier: Option<String>,
    pub urgent: bool,
}

impl Overrides {
    pub fn set(&mut self, flag: &str, value: &str) -> bool {
        let slot = match flag {
            "--local" => &mut self.local,
            "--pin" => &mut self.pin,
            "--tier-reason" => &mut self.tier,
            _ => return false,
        };
        *slot = Some(value.to_owned());
        true
    }

    /// A revived session keeps the reasons its launch recorded unless new ones are given.
    pub fn recorded(mut self, note: Option<&str>) -> Self {
        for part in note.unwrap_or_default().split("; ") {
            match part.split_once(": ") {
                Some(("local", reason)) if self.local.is_none() => self.local = Some(reason.into()),
                Some(("pin", reason)) if self.pin.is_none() => self.pin = Some(reason.into()),
                Some(("tier", reason)) if self.tier.is_none() => self.tier = Some(reason.into()),
                _ => {}
            }
        }
        self
    }
}

/// Where placement admitted the launch: the override note to record and the machine to run on, if not here.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Placed {
    pub account: Option<String>,
    pub note: Option<String>,
    pub machine: Option<Machine>,
    pub need: Option<String>,
    /// A model the decision overrides, such as the coordinator's Opus high when no account has Fable headroom.
    pub model: Option<String>,
    pub effort: Option<Effort>,
}

#[derive(Clone, Copy)]
pub(super) struct Launch<'a> {
    pub name: &'a str,
    pub account: &'a str,
    pub brief: Option<&'a Path>,
    /// The directory a fresh session starts in, which a machine must also have; None on revive,
    /// which resumes where the session already ran.
    pub cwd: Option<&'a Path>,
    /// The machine `--cwd HOST:PATH` or the revived record names, which placement must run the box on.
    pub machine: Option<&'a str>,
    /// A fresh launch of a lead, which staffs other hosts and so runs only where its commands have the reach to launch there.
    pub lead: bool,
    pub role: Role,
    /// The model the launch records, whose family decides which meter placement reads.
    pub model: Option<&'a str>,
    pub family: ModelFamily,
}

/// The brief placement reads: the system prompt file when there is one, then the prompt.
pub(super) fn brief_text(system_prompt: Option<&Path>, prompt: &str) -> NorthResult<String> {
    let file = system_prompt.map(std::fs::read_to_string).transpose()?.unwrap_or_default();
    Ok(format!("{file}\n{prompt}"))
}

/// Why launch `name` placed on another machine may not start: `written`, its prompt as the caller
/// wrote it, names a path under the launching host's home, which only that host can read, other
/// than one inside `cwd`, the directory the session starts in, which the machine has. Text the
/// launch adds itself, such as another session's prior report naming its own lane, sends the worker
/// to nothing and is not read. The refusal names each path so the launch can inline the text or pass
/// it with `--system-prompt`, which the launch reads here.
pub(super) fn home_path_refusal(name: &str, written: &str, cwd: &Path, machine: Option<&Machine>) -> Option<String> {
    let machine = machine.filter(|machine| machine.name != crate::platform::host::name())?;
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    let paths = crate::platform::home_paths::outside(written, Path::new(&home), cwd);
    (!paths.is_empty()).then(|| {
        let named: Vec<String> = paths.iter().map(|path| path.display().to_string()).collect();
        format!(
            "{} would start on {}, but its prompt names {}, under this host's home, which {} cannot read; inline the text or pass it with --system-prompt, which the launch reads here",
            name,
            machine.name,
            named.join(", "),
            machine.name,
        )
    })
}

/// Placement gates every Claude launch except `--test` sessions, before anything starts, on the brief and the `needs` the item it names carries; a
/// harness launch (DeepSeek on opencode) is gated on its host's saturation alone. `report`
/// prints its verdict lines; `written` is the prompt as the caller wrote it, before the launch added its own lines. A refusal
/// is a loss row written here, by the gate that saw it, since a launch that never starts leaves no run for the ledger to charge.
pub(super) fn placed(fleet: &Fleet, request: &LaunchRequest, written: &str, needs: &[String], overrides: &Overrides, report: bool) -> NorthResult<Result<Placed, Refusal>> {
    let settled = place(fleet, request, written, needs, overrides, report)?;
    if let Err(refusal) = &settled {
        attribution::record(&request.name, "launch", Cause::Harness(Harness::LaunchRefusal), &refusal.to_string());
    }
    Ok(settled)
}

fn place(fleet: &Fleet, request: &LaunchRequest, written: &str, needs: &[String], overrides: &Overrides, report: bool) -> NorthResult<Result<Placed, Refusal>> {
    if let Some(why) = home_path_refusal(&request.name, written, &request.cwd, request.machine.as_ref()) {
        return Ok(Err(Refusal::Final(why)));
    }
    if request.verification {
        return Ok(Ok(Placed::default()));
    }
    if request.provider.label() != "claude" {
        let host = request.machine.as_ref().map_or_else(|| fleet.host().to_owned(), |machine| machine.name.clone());
        if request.provider.label() == crate::dispatch::deepseek::PROVIDER {
            let running = crate::dispatch::deepseek::cap::running(fleet.sessions()?.iter().filter(|record| fleet.running(record)).map(|record| record.provider.label()));
            if let Err(why) = crate::dispatch::deepseek::cap::admit(running, crate::dispatch::deepseek::Settings::read().slot_cap) {
                return Ok(Err(Refusal::Saturated(why)));
            }
        }
        if let Some(refusal) = Refusal::line_hold(request.role() == Role::Worker, request.urgent) {
            return Ok(Err(refusal));
        }
        return Ok(harness_host(&host, crate::pacing::placement::saturation_reading(&host).ok().as_ref()));
    }
    let overrides = &Overrides { urgent: request.urgent, ..overrides.clone() };
    let text = crate::pacing::placement::needs::with_needs(&gate::anchored(&brief_text(request.system_prompt.as_deref(), &request.prompt)?, Some(&request.cwd), request.task().as_deref()), needs);
    let model = request.model.as_deref();
    let launch = Launch {
        name: &request.name,
        account: &request.account,
        brief: request.system_prompt.as_deref(),
        cwd: Some(&request.cwd),
        machine: request.machine.as_ref().map(|machine| machine.name.as_str()),
        lead: is_lead(request),
        role: request.role(),
        model,
        family: family_of(model),
    };
    if let Some(why) = lead_refusal(fleet.host(), &launch, |host| trusted(host, fleet.host())) {
        return Ok(Err(Refusal::Final(why)));
    }
    let (roster, load) = (fleet.roster(), machine_load(fleet)?);
    let (placed, view) = std::thread::scope(|scope| {
        let view = scope.spawn(|| crate::dispatch::spend_now(&roster));
        let placed = check(&launch, &text, overrides, &load, &roster, report);
        (placed, view.join().unwrap_or_else(|_| Err("its read panicked".to_owned())))
    });
    let placed = placed?;
    if let Ok(placed) = &placed
        && let Some(why) = home_path_refusal(&request.name, written, &request.cwd, placed.machine.as_ref())
    {
        return Ok(Err(Refusal::Final(why)));
    }
    if placed.is_err() {
        return Ok(placed);
    }
    let starter = if request.role == Some(Role::Proxy) || text.contains("[routine:") { Starter::Reserved } else { Starter::Worker };
    let view = match view {
        Ok(read) => read,
        Err(why) => return Ok(Err(Refusal::Spend(format!("spend view unread: {why}")))),
    };
    let admits = |account: &str| view.admit(&format!("claude:{account}"), starter).map_err(|refusal| refusal.to_string());
    let chosen = placed.as_ref().ok().and_then(|placed| placed.account.clone()).unwrap_or_else(|| request.account.clone());
    let others: Vec<&str> = view.accounts.iter().filter_map(|other| other.name.strip_prefix("claude:")).collect();
    let settled = if request.account.is_empty() {
        with_room(&chosen, &others, admits, |other| check(&Launch { account: other, ..launch }, &text, overrides, &load, &roster, false), placed)?
    } else {
        admits(&chosen).map_err(Refusal::Spend).and(placed)
    };
    Ok(settled)
}

/// A launch of a non-Claude harness (DeepSeek on opencode) obeys the host verdict a Claude launch obeys: a host that is
/// saturated by memory, disk, load or admission queue starts nothing, and an unreadable reading refuses nothing.
pub(super) fn harness_host(host: &str, reading: Option<&crate::capacity::saturation::Saturation>) -> Result<Placed, Refusal> {
    match reading.and_then(|reading| reading.refusal(host)) {
        Some(why) => Err(Refusal::Saturated(format!("{why}; it starts there once the host has room"))),
        None => Ok(Placed::default()),
    }
}

/// Whether the spend rule admits one more session on the request's own account: what a tick's hand-over checks before it starts a session elsewhere.
pub(super) fn spend_admits(fleet: &Fleet, request: &LaunchRequest) -> NorthResult<()> {
    let starter = if request.role == Some(Role::Proxy) { Starter::Reserved } else { Starter::Worker };
    let view = crate::dispatch::spend_now(&fleet.roster()).map_err(|why| NorthError::Saturated(format!("spend view unread: {why}")))?;
    let account = format!("{}:{}", request.provider.label(), request.account);
    view.admit(&account, starter).map_err(|refusal| NorthError::Saturated(refusal.to_string()))
}

/// The placement `chosen` when the spend rule admits its account; otherwise the first other account the rule admits that placement also
/// accepts, so a launch that named none starts where a worker session fits. With none left the refusal names each account's reason.
fn with_room(
    chosen: &str,
    accounts: &[&str],
    admits: impl Fn(&str) -> Result<(), String>,
    mut place_on: impl FnMut(&str) -> NorthResult<Result<Placed, Refusal>>,
    placed: Result<Placed, Refusal>,
) -> NorthResult<Result<Placed, Refusal>> {
    let Err(first) = admits(chosen) else { return Ok(placed) };
    let mut refusals = vec![first];
    for other in accounts.iter().filter(|other| **other != chosen) {
        match admits(other) {
            Ok(()) => {
                match place_on(other)? {
                    Ok(settled) => return Ok(Ok(Placed { account: Some((*other).to_owned()), ..settled })),
                    Err(line @ Refusal::Line(_)) => return Ok(Err(line)),
                    Err(_) => {}
                }
            }
            Err(why) => refusals.push(why),
        }
    }
    Ok(Err(Refusal::Spend(refusals.join("; "))))
}

/// Runs live placement, prints one verdict line per account and machine when `report`, and returns where to run.
/// `fleet_load` counts the live fleet sessions this machine runs on each other machine.
pub(super) fn check(launch: &Launch, text: &str, overrides: &Overrides, fleet_load: &BTreeMap<String, i64>, roster: &Roster, report: bool) -> NorthResult<Result<Placed, Refusal>> {
    let ask = case::Ask { launch: recorded_launch(launch, overrides), brief: text, fleet_load, roster };
    let mut case = case::gather(&ask).map_err(configuration)?;
    let turn = recent::Turn::take();
    recent::charge(&mut case);
    let (decided, decide_us) = decision_log::timed(|| decide(&case, launch, overrides).map(|decision| {
        let settled = settle(&decision, launch, text, overrides);
        (decision, settled)
    }));
    let (decision, settled) = decided?;
    if report {
        eprintln!("placement: {}", gate::summary(&decision));
        for line in decision.verdicts.iter().map(|verdict| &verdict.line).chain(&decision.machine_lines) {
            eprintln!("  {line}");
        }
    }
    decision_log::record(decision_log::Entry { kind: Kind::Placement, at: case.at, follows: None, case: &case, decision: &recorded_decision(&decision, &settled), decide_us });
    drop(turn);
    Ok(settled)
}

fn configuration(error: String) -> NorthError {
    NorthError::Configuration(format!("placement: {error}"))
}

fn recorded_launch(launch: &Launch, overrides: &Overrides) -> record::Launch {
    let path = |path: &Path| path.display().to_string();
    record::Launch {
        name: launch.name.to_owned(),
        account: launch.account.to_owned(),
        fresh: launch.cwd.is_some(),
        cwd: launch.cwd.map(path),
        named_machine: launch.machine.map(str::to_owned),
        overrides: record::Overrides { local: overrides.local.clone(), pin: overrides.pin.clone(), tier: overrides.tier.clone(), urgent: overrides.urgent },
        provider: "claude".to_owned(),
        verification: false,
        model: launch.model.map(str::to_owned),
        role: Some(launch.role.label().to_owned()),
    }
}

fn recorded_decision(decision: &Decision, settled: &Result<Placed, Refusal>) -> record::Decision {
    let target = case::recorded_target(&decision.target);
    let outcome = match settled {
        Ok(placed) => record::Outcome::Placed { account: placed.account.clone(), note: placed.note.clone(), machine: placed.machine.as_ref().map(|machine| machine.name.clone()), need: placed.need.clone() },
        Err(refusal) => record::Outcome::Refused { kind: refusal.kind(), why: refusal.to_string() },
    };
    let lines = std::iter::once(gate::summary(decision)).chain(decision.verdicts.iter().map(|verdict| verdict.line.clone())).chain(decision.machine_lines.iter().cloned()).collect();
    record::Decision { target, machine: decision.machine.as_ref().map(|machine| machine.name.clone()), outcome, lines }
}

pub(super) fn settle(decision: &Decision, launch: &Launch, text: &str, overrides: &Overrides) -> Result<Placed, Refusal> {
    if let Some(why) = &decision.line_held {
        return Err(Refusal::Line(why.clone()));
    }
    if !launch.account.is_empty() {
        return admit(decision, launch, text, overrides);
    }
    let account = pick(decision, overrides)?;
    let mut placed = admit(decision, &Launch { account: &account, ..*launch }, text, overrides)?;
    if let Some(verdict) = decision.verdict(&format!("claude:{account}")) {
        let source = format!("source: {account}, {}", verdict.rank.reason());
        placed.note = Some(placed.note.map_or_else(|| source.clone(), |note| format!("{note}; {source}")));
    }
    placed.account = Some(account);
    Ok(placed)
}

fn pick(decision: &Decision, overrides: &Overrides) -> Result<String, Refusal> {
    match (&decision.target, &overrides.local) {
        (Target::Held(why), _) if decision.held_for_room() => Err(Refusal::Saturated(format!("held: {why}"))),
        (Target::Held(why), _) => Err(Refusal::Final(format!("held: {why}"))),
        (Target::Unserved(_), _) | (Target::Cloud(_), None) => Ok(String::new()),
        _ => picked(decision)
            .map(|verdict| verdict.account.trim_start_matches("claude:").to_owned())
            .ok_or_else(|| Refusal::Final("no Claude account is available; wait for a window to reset".to_owned())),
    }
}

fn decide(case: &Case, launch: &Launch, overrides: &Overrides) -> NorthResult<Decision> {
    let mut decision = case::decide(case).map_err(configuration)?;
    fall_through(&mut decision, launch, overrides);
    let absent = match (&decision.machine, launch.cwd) {
        (Some(machine), Some(cwd)) if lacks_directory(case, &machine.name) => Some(format!("{} has no {}, so here", machine.name, cwd.display())),
        _ => None,
    };
    if absent.is_some() {
        decision.stay_here(absent);
    }
    if launch.lead && let Some(machine) = decision.machine.as_ref().filter(|machine| machine.reaches.as_ref().is_none_or(|reached| reached.is_empty())) {
        let line = format!("{} is a worker host: {LEAD_ON_TRUSTED}, so the lead stays here", machine.name);
        decision.stay_here(Some(line));
    }
    decision.hold_here();
    Ok(decision)
}

/// A fresh launch without `--pin` or `--local` falls through a saturated machine it named; a revive stays where its conversation is.
pub(super) fn fall_through(decision: &mut Decision, launch: &Launch, overrides: &Overrides) {
    if launch.cwd.is_some() && overrides.pin.is_none() && overrides.local.is_none() {
        decision.fall_through(launch.account);
    }
}

/// Ok(where) when the launch may start, Err(refusal with the way forward) otherwise. `--local` keeps
/// the box here instead of the cloud or another machine.
pub(super) fn admit(decision: &Decision, launch: &Launch, text: &str, overrides: &Overrides) -> Result<Placed, Refusal> {
    let mut notes = Vec::new();
    let elsewhere = matches!(decision.target, Target::Cloud(_)) || decision.machine.is_some();
    match (&decision.target, &overrides.local) {
        (Target::Unserved(why), _) => return Err(Refusal::Final(format!("{why}; list it in a holding host's [hosts.NAME] serves"))),
        (Target::Cloud(lane), None) => return Err(Refusal::Final(cloud_command(launch, lane, &decision.here, &decision.surface, text))),
        (_, Some(reason)) if elsewhere || decision.here_saturated.is_some() => notes.push(format!("local: {reason}")),
        _ => {}
    }
    let machine = match (&decision.machine, &overrides.local) {
        (Some(chosen), None) if !chosen.accounts.iter().any(|label| label == launch.account) => return Err(Refusal::Final(not_signed_in(decision, chosen, launch.account))),
        (Some(chosen), None) => Some(Machine { name: chosen.name.clone(), ssh: chosen.ssh.clone(), agent_ssh: chosen.agent_ssh.clone() }),
        _ => None,
    };
    notes.extend(decision.moved.clone().filter(|_| machine.is_some()));
    if let Some(out) = limit_refusal(decision, launch.account) {
        return Err(Refusal::Spend(out));
    }
    let saturated = decision.saturated.as_ref().filter(|_| machine.is_some()).map(|why| format!("{why}; it starts there once the host has room, or launch it elsewhere, or here with --local \"<reason>\""));
    let hot = decision.here_saturated.as_ref().filter(|_| machine.is_none() && overrides.local.is_none()).map(|why| format!("{why}; it starts here once the host has room, or launch from another host, or keep it here with --local \"<reason>\""));
    if let Some(full) = saturated.or(hot) {
        return Err(Refusal::Saturated(full));
    }
    let cpu = decision.here_cpu_held.as_ref().filter(|_| machine.is_none()).map(|why| format!("{why} To start it here anyway, launch with --pin \"<reason>\"."));
    let on_battery = decision.here_on_battery.clone().filter(|_| overrides.local.is_some()).map(Refusal::Final);
    let held = cpu.map(Refusal::Saturated).or(on_battery);
    if let Some(refusal) = held {
        let Some(reason) = &overrides.pin else {
            return Err(refusal);
        };
        notes.push(format!("pin: {reason}"));
    }
    if let Some(reason) = &overrides.tier {
        notes.push(format!("tier: {reason}"));
    }
    let (model, effort) = match decision.family.filter(|family| is_coordinator(launch.role) && *family != launch.family) {
        Some(ModelFamily::Opus) => {
            notes.push("model: Opus high, no account has Fable headroom".to_owned());
            (Some("opus".to_owned()), Some(Effort::High))
        }
        _ => (None, None),
    };
    Ok(Placed { account: None, note: (!notes.is_empty()).then(|| notes.join("; ")), machine, need: gate::capacity_need(text), model, effort })
}

fn not_signed_in(decision: &Decision, machine: &gate::Machine, account: &str) -> String {
    let there = if machine.accounts.is_empty() { "none".to_owned() } else { machine.accounts.join(", ") };
    let instead = match &decision.target {
        Target::Account(other) if other.trim_start_matches("claude:") != account => format!("launch with --account {}, or ", other.trim_start_matches("claude:")),
        _ => String::new(),
    };
    format!("{} is not signed in to claude:{account} (signed in there: {there}); {instead}keep the box here with --local \"<reason>\"", machine.name)
}

/// An account at its provider's limit cannot run the box at all, so no `--pin`, recorded or given, keeps it.
fn limit_refusal(decision: &Decision, account: &str) -> Option<String> {
    let chosen = decision.verdict(&format!("claude:{account}")).filter(|verdict| verdict.exhausted)?;
    let instead = match picked(decision) {
        Some(better) => format!("placement picks {} ({}); launch with --account {}", better.account, better.line, better.account.trim_start_matches("claude:")),
        None => "no Claude account is available; wait for a window to reset".to_owned(),
    };
    Some(format!("provider limit: {}; {instead}", chosen.line))
}

fn picked(decision: &Decision) -> Option<&gate::Verdict> {
    match &decision.target {
        Target::Account(placed) => decision.verdict(placed),
        _ => decision.best_here(),
    }
}

/// Whether the per-machine probe found the launch directory missing on `machine`; a probe that did not answer about it leaves the host to refuse it.
fn lacks_directory(case: &Case, machine: &str) -> bool {
    case.machines.iter().find(|probed| probed.name == machine).and_then(|probed| probed.has_cwd) == Some(false)
}

fn cloud_command(launch: &Launch, lane: &CloudLane, here: &str, surface: &str, text: &str) -> String {
    let brief = launch.brief.map_or_else(|| "BRIEF.md".to_owned(), |path| path.display().to_string());
    let repo = gate::repository(text);
    let branch = cloud_branch(launch.name);
    let landing = if cloud_autoland(&repo) {
        format!("Autoland lands {branch} once its checks pass, so no courier runs.")
    } else {
        format!(
            "Land it from gex45 or the laptop, never Nexus: once git ls-remote origin {branch} shows it, fetch it into a worktree, rerun the checks, land it with safe-push --to main and delete it with git push origin --delete {branch}. \
             If the session reports its push refused, north cloud wait <id> OUT.jsonl run from a north checkout on gex45 or the laptop lands its handback through north cloud land, as cloud.md says."
        )
    };
    let label = lane.account.trim_start_matches("claude:");
    let login = format!("CLAUDE_CONFIG_DIR=$HOME/.local/state/north/profiles/claude-{label}");
    let name = launch.name;
    let (copy, start) = if lane.host == here {
        (format!("mkdir -p /tmp/cloud && cp {brief} /tmp/cloud/{name}.md"), format!("{login} north cloud launch {repo} {name} /tmp/cloud/{name}.md"))
    } else {
        (
            format!("ssh {host} 'mkdir -p /tmp/cloud && cat > /tmp/cloud/{name}.md' < {brief}", host = lane.host),
            format!("ssh {host} '{login} north cloud launch {repo} {name} /tmp/cloud/{name}.md'", host = lane.host),
        )
    };
    format!(
        "placement sends this box to anthropic-cloud under {account} ({credit}; {surface}); cloud compute has no separate charge and the session draws {account}'s plan limits. Run it on {host}, where {account} is signed in (harness/skills/workers/references/cloud.md):\n  \
         {copy}\n  \
         {start}\n\
         It launches claude --cloud from /tmp/cloud/src/{repo}, adds the cloud safety line and the steps that push {branch}, and prints the session_... ID; record it in the box's thread.\n\
         {landing}\n\
         To run it here anyway, relaunch with --local \"<reason>\".",
        account = lane.account,
        credit = lane.credit,
        host = lane.host,
    )
}

/// Live fleet sessions per machine they run on, which those machines cannot count themselves.
pub(super) fn machine_load(fleet: &Fleet) -> NorthResult<BTreeMap<String, i64>> {
    let mut load = BTreeMap::new();
    for record in fleet.sessions()? {
        if let Some(machine) = record.machine.as_ref().filter(|_| fleet.running(&record)) {
            *load.entry(machine.name.clone()).or_default() += 1;
        }
    }
    Ok(load)
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "placement_burst_tests.rs"]
mod burst_tests;

/// Where a fresh session starts: `HOST:/PATH` names a `[hosts.HOST]` machine and a directory there,
/// anything else is a directory here; none is the current directory.
pub(super) fn working_directory(cwd: Option<PathBuf>) -> NorthResult<(PathBuf, Option<Machine>)> {
    let Some(cwd) = cwd else { return Ok((std::env::current_dir()?, None)) };
    let named = cwd.to_str().and_then(|text| text.split_once(':')).filter(|(host, path)| !host.contains('/') && path.starts_with('/'));
    let Some((host, path)) = named else { return Ok((std::fs::canonicalize(cwd)?, None)) };
    let machine = crate::pacing::placement::machine(host).ok_or_else(|| NorthError::Usage(format!("--cwd {host}:{path} names no [hosts.{host}] with an ssh destination")))?;
    Ok((PathBuf::from(path), Some(machine)))
}

const LEAD_ON_TRUSTED: &str = "a lead staffs other hosts and its commands run on the host that runs it, so it runs on a host that places sessions on the others (the laptop or nexus)";

/// Whether the session this launch starts is a lead as its record will say: it names no parent and holds no task.
fn is_lead(request: &LaunchRequest) -> bool {
    request.role.map_or_else(|| request.parent.is_none() && request.tasks().is_empty() && request.name != crate::fleet::runtime::PROXY, |role| role == Role::Lead)
}

/// The family whose meter placement reads: the launch model's, else the shared Opus windows.
fn family_of(model: Option<&str>) -> ModelFamily {
    model.and_then(crate::usage::meter::family).unwrap_or(ModelFamily::Opus)
}

/// Whether the role is the fleet coordinator, by either spelling the role has had.
fn is_coordinator(role: Role) -> bool {
    matches!(role.label(), "proxy" | "coordinator")
}

/// Whether `host` is trusted: it holds the ledger or declares the hosts it `reaches`; a worker host declares none.
fn trusted(host: &str, here: &str) -> bool {
    let ledger = crate::threads::client::ledger_host_name();
    let reaches = crate::pacing::placement::config().ok().and_then(|cfg| cfg.get("hosts")?.get(host)?.get("reaches")?.as_array().map(|hosts| !hosts.is_empty()));
    reaches.unwrap_or(false) || ledger.as_deref() == Some(host) || (host == here && ledger.is_none())
}

/// Why a fresh lead may not run on `here`'s own registry or on the worker machine it names, None for any other launch.
fn lead_refusal(here: &str, launch: &Launch, trusted: impl Fn(&str) -> bool) -> Option<String> {
    if !launch.lead || launch.cwd.is_none() {
        return None;
    }
    let worker = if trusted(here) { launch.machine.filter(|machine| !trusted(machine))? } else { here };
    Some(format!("{worker} is a worker host: {LEAD_ON_TRUSTED}; launch {} from the laptop or nexus", launch.name))
}
