use std::collections::BTreeMap;
use std::path::Path;

use super::admit::{fall_through, settle};
use super::case;
use super::gate::{self, Decision};
use super::launch::{LEAD_ON_TRUSTED, Launch, Overrides, Placed, brief_text, family_of, home_path_refusal, is_lead, lead_refusal, machine_load, trusted};
use super::recent;
use super::refusal::Refusal;
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Role};
use crate::fleet::registry::Fleet;
use crate::fleet::roster::Roster;
use crate::platform::attribution::{self, Cause, Harness};
use crate::platform::decision_log;
use crate::spend::Starter;
use north_strategy_types::Kind;
use north_strategy_types::placement::{self as record, Case};

/// Launch placement over `fleet`, with the two reads that live in the dispatch crate above this one: the fleet-wide spend view and the harness slot cap.
#[derive(Clone, Copy)]
pub struct Placer<'a> {
    pub fleet: &'a Fleet,
    pub spend_now: fn(&Roster) -> Result<crate::spend::View, String>,
    pub harness_cap: fn(&Fleet, &LaunchRequest) -> Result<(), String>,
}

impl Placer<'_> {
    fn place(&self, request: &LaunchRequest, written: &str, needs: &[String], overrides: &Overrides, report: bool, operator: bool) -> NorthResult<Result<Placed, Refusal>> {
        let fleet = self.fleet;
        if let Some(why) = home_path_refusal(&request.name, written, &request.cwd, request.machine.as_ref()) {
            return Ok(Err(Refusal::Final(why)));
        }
        if request.verification {
            return Ok(Ok(Placed::default()));
        }
        if request.provider.label() != "claude" {
            let host = request.machine.as_ref().map_or_else(|| fleet.host().to_owned(), |machine| machine.name.clone());
            if let Err(why) = (self.harness_cap)(fleet, request) {
                return Ok(Err(Refusal::Saturated(why)));
            }
            if let Some(refusal) = Refusal::line_hold(request.role() == Role::Worker) {
                return Ok(Err(refusal));
            }
            return Ok(harness_host(&host, super::saturation_reading(&host).ok().as_ref()));
        }
        let overrides = &Overrides { urgent: request.urgent, ..overrides.clone() };
        let text = super::needs::with_needs(&gate::anchored(&brief_text(request.system_prompt.as_deref(), &request.prompt)?, Some(&request.cwd), request.task().as_deref()), needs);
        let repository = gate::launch_repository(Some(&request.cwd), request.task().as_deref());
        let model = request.model.as_deref();
        let launch = Launch { name: &request.name, account: &request.account, brief: request.system_prompt.as_deref(), cwd: Some(&request.cwd), machine: request.machine.as_ref().map(|machine| machine.name.as_str()), lead: is_lead(request), role: request.role(), model, family: family_of(model), operator, repository: &repository };
        if let Some(why) = lead_refusal(fleet.host(), &launch, |host| trusted(host, fleet.host())) {
            return Ok(Err(Refusal::Final(why)));
        }
        let (roster, load) = (fleet.roster(), machine_load(fleet)?);
        let (placed, view) = std::thread::scope(|scope| {
            let view = scope.spawn(|| (self.spend_now)(&roster));
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
        let settled = if request.account.is_empty() { with_room(&chosen, &others, admits, |other| check(&Launch { account: other, ..launch }, &text, overrides, &load, &roster, false), placed)? } else { admits(&chosen).map_err(Refusal::Spend).and(placed) };
        Ok(settled)
    }

    /// Placement gates every Claude launch except `--test` sessions, before anything starts, on the brief and the `needs` the item it names carries; a
    /// harness launch (DeepSeek on opencode) is gated on its host's saturation alone. `report`
    /// prints its verdict lines; `written` is the prompt as the caller wrote it, before the launch added its own lines. A refusal
    /// is a loss row written here, by the gate that saw it, since a launch that never starts leaves no run for the ledger to charge.
    pub fn placed(&self, request: &LaunchRequest, written: &str, needs: &[String], overrides: &Overrides, report: bool, operator: bool) -> NorthResult<Result<Placed, Refusal>> {
        let settled = self.place(request, written, needs, overrides, report, operator)?;
        if let Err(refusal) = &settled {
            attribution::record(&request.name, "launch", Cause::Harness(Harness::LaunchRefusal), &refusal.to_string());
        }
        Ok(settled)
    }
}

/// A launch of a non-Claude harness (DeepSeek on opencode) obeys the host verdict a Claude launch obeys: a host that is
/// saturated by memory, disk, load or admission queue starts nothing, and an unreadable reading refuses nothing.
pub fn harness_host(host: &str, reading: Option<&crate::capacity::saturation::Saturation>) -> Result<Placed, Refusal> {
    match reading.and_then(|reading| reading.refusal(host)) {
        Some(why) => Err(Refusal::Saturated(format!("{why}; it starts there once the host has room"))),
        None => Ok(Placed::default()),
    }
}

/// The placement `chosen` when the spend rule admits its account; otherwise the first other account the rule admits that placement also
/// accepts, so a launch that named none starts where a worker session fits. With none left the refusal names each account's reason.
pub fn with_room(chosen: &str, accounts: &[&str], admits: impl Fn(&str) -> Result<(), String>, mut place_on: impl FnMut(&str) -> NorthResult<Result<Placed, Refusal>>, placed: Result<Placed, Refusal>) -> NorthResult<Result<Placed, Refusal>> {
    let Err(first) = admits(chosen) else {
        return Ok(placed);
    };
    let mut refusals = vec![first];
    for other in accounts.iter().filter(|other| **other != chosen) {
        match admits(other) {
            Ok(()) => match place_on(other)? {
                Ok(settled) => {
                    return Ok(Ok(Placed { account: Some((*other).to_owned()), ..settled }));
                }
                Err(line @ Refusal::Line(_)) => return Ok(Err(line)),
                Err(_) => {}
            },
            Err(why) => refusals.push(why),
        }
    }
    Ok(Err(Refusal::Spend(refusals.join("; "))))
}

/// Runs live placement, prints one verdict line per account and machine when `report`, and returns where to run.
/// `fleet_load` counts the live fleet sessions this machine runs on each other machine.
pub fn check(launch: &Launch, text: &str, overrides: &Overrides, fleet_load: &BTreeMap<String, i64>, roster: &Roster, report: bool) -> NorthResult<Result<Placed, Refusal>> {
    let ask = case::Ask { launch: recorded_launch(launch, overrides), brief: text, fleet_load, roster };
    let mut case = case::gather(&ask).map_err(configuration)?;
    let turn = recent::Turn::take();
    recent::charge(&mut case);
    let (decided, decide_us) = decision_log::timed(|| {
        decide(&case, launch, overrides).map(|decision| {
            let settled = settle(&decision, launch, text, overrides);
            (decision, settled)
        })
    });
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
    record::Launch { name: launch.name.to_owned(), account: launch.account.to_owned(), fresh: launch.cwd.is_some(), cwd: launch.cwd.map(path), named_machine: launch.machine.map(str::to_owned), overrides: record::Overrides { local: overrides.local.clone(), pin: overrides.pin.clone(), tier: overrides.tier.clone(), urgent: overrides.urgent }, provider: "claude".to_owned(), verification: false, model: launch.model.map(str::to_owned), role: Some(launch.role.label().to_owned()), operator: launch.operator, repository: launch.repository.to_owned() }
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

pub fn decide(case: &Case, launch: &Launch, overrides: &Overrides) -> NorthResult<Decision> {
    let mut decision = case::decide(case).map_err(configuration)?;
    fall_through(&mut decision, launch, overrides);
    let absent = match (&decision.machine, launch.cwd) {
        (Some(machine), Some(cwd)) if lacks_directory(case, &machine.name) => Some(format!("{} has no {}, so here", machine.name, cwd.display())),
        _ => None,
    };
    if absent.is_some() {
        decision.stay_here(absent);
    }
    if launch.lead
        && let Some(machine) = decision.machine.as_ref().filter(|machine| machine.reaches.as_ref().is_none_or(|reached| reached.is_empty()))
    {
        let line = format!("{} is a worker host: {LEAD_ON_TRUSTED}, so the lead stays here", machine.name);
        decision.stay_here(Some(line));
    }
    decision.hold_here();
    Ok(decision)
}

/// Whether the per-machine probe found the launch directory missing on `machine`; a probe that did not answer about it leaves the host to refuse it.
pub fn lacks_directory(case: &Case, machine: &str) -> bool {
    case.machines.iter().find(|probed| probed.name == machine).and_then(|probed| probed.has_cwd) == Some(false)
}
