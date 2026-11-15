//! Who hears a launched session's reports: the calling fleet session, else the coordinator for a
//! launch from outside the fleet (dispatch, timers, a bare terminal), unless `--parent` names
//! another; `none` is the coordinator's and a `--test` probe's alone.
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Role};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::COORDINATOR;

/// Whether a launch from outside the fleet can fall back to the coordinator, saying so when its reports would reach nobody.
pub(super) fn coordinator_reachable(fleet: &Fleet, caller: Option<&str>, words: &[&str]) -> bool {
    let reached = fleet.reaches(COORDINATOR);
    if let Err(error) = &reached
        && outside(caller)
        && !words.contains(&"--parent")
    {
        eprintln!("fleet launch: a launch from outside the fleet has no parent, so its reports reach nobody: {}", error.user_message());
    }
    reached.is_ok()
}

/// The parent a launch reports to; the coordinator is the one root of the org, so `--parent none` is the coordinator's own and a `--test` probe's.
pub(super) fn parent(flag: Option<String>, caller: Option<String>, coordinator: bool, name: &str, role: Option<Role>, probe: bool) -> NorthResult<Option<String>> {
    let is_coordinator = role == Some(Role::Coordinator) || name == COORDINATOR;
    Ok(match flag.as_deref() {
        Some("none") if is_coordinator || probe => None,
        Some("none") => {
            return Err(NorthError::Usage(format!(
                "--parent none leaves {name} outside the org, whose one root is the coordinator; drop it so {name} reports to its caller or the coordinator, or name another --parent"
            )));
        }
        Some(_) => flag,
        None if is_coordinator => None,
        None => caller.filter(|caller| !caller.is_empty()).or_else(|| coordinator.then(|| COORDINATOR.to_owned())),
    })
}

/// `--wait` or `--no-wait` when given; a launch from outside the fleet does not hold for admission under its fallback parent.
pub(super) fn wait(flag: Option<bool>, parent: Option<&str>, caller: Option<&str>) -> Option<bool> {
    flag.or((parent.is_none() && outside(caller)).then_some(false))
}

/// `--dispatched`, which marks a launch `north fleet dispatch` makes from outside the fleet, so a fleet session's launch is refused it.
pub(super) fn dispatched(asked: bool, caller: Option<&str>) -> NorthResult<bool> {
    if asked && !outside(caller) {
        return Err(NorthError::Refused("--dispatched marks a launch north fleet dispatch makes, so a fleet session's launch never carries it".to_owned()));
    }
    Ok(asked)
}

fn outside(caller: Option<&str>) -> bool {
    caller.is_none_or(str::is_empty)
}

pub(super) fn reader(fleet: &Fleet, request: &mut LaunchRequest, caller: Option<&str>) -> NorthResult<()> {
    if request.verification || request.role() != Role::Worker {
        return Ok(());
    }
    match request.parent.as_deref() {
        Some(parent) if fleet.listens(parent)? => Ok(()),
        Some(parent) => Err(NorthError::Refused(format!("{parent} is not a live fleet session, so the reports of worker {} would reach nobody; name a live --parent or launch it from your own fleet session", request.name))),
        None => match caller.filter(|caller| !caller.is_empty()) {
            Some(caller) if fleet.listens(caller)? => {
                request.parent = Some(caller.to_owned());
                Ok(())
            }
            _ => Err(NorthError::Refused(format!("worker {} has no parent to read its reports; launch it from a live fleet session, which becomes its parent, or name a live --parent", request.name))),
        },
    }
}

pub(super) fn from_coordinator(fleet: &Fleet, caller: Option<&str>) -> bool {
    caller.is_some_and(|caller| caller == COORDINATOR || fleet.role_of(caller) == Some(Role::Coordinator))
}

/// A launch from a lead or worker starts a worker unless it names the lead role, and holds its launcher's task when it names none; a lead's take-over of a lead seat keeps that seat's lead role.
pub(super) fn inherit(fleet: &Fleet, request: &mut LaunchRequest, caller: Option<&str>, take_over: bool) -> NorthResult<()> {
    match caller {
        Some(caller) => {
            let seat = take_over.then(|| fleet.role_of(&request.name)).flatten();
            inherit_from(request, fleet.role_of(caller), fleet.task_of(caller), seat)
        }
        None => Ok(()),
    }
}

fn inherit_from(request: &mut LaunchRequest, launcher: Option<Role>, task: Option<String>, seat: Option<Role>) -> NorthResult<()> {
    let Some(launcher @ (Role::Lead | Role::Worker)) = launcher else {
        return Ok(());
    };
    if let Some(named) = request.role
        && (matches!(named, Role::Coordinator | Role::Reviewer) || (launcher == Role::Worker && named == Role::Lead))
    {
        return Err(NorthError::Refused(format!("a {launcher:?} session cannot launch {} as a {named:?}; drop --role or name a role below your own", request.name)));
    }
    let kept = if launcher == Role::Lead && seat == Some(Role::Lead) { Role::Lead } else { Role::Worker };
    request.role.get_or_insert(kept);
    if request.tasks().is_empty() {
        request.task_ref = task;
    }
    Ok(())
}

#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
