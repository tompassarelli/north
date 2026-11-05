//! Who hears a launched session's reports: the calling fleet session, else the proxy for a
//! launch from outside the fleet (dispatch, timers, a bare terminal), unless `--parent` names
//! another or `none`.
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Role};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::PROXY;

/// Whether a launch from outside the fleet can fall back to the proxy, saying so when its reports would reach nobody.
pub(super) fn proxy_reachable(fleet: &Fleet, caller: Option<&str>, words: &[&str]) -> bool {
    let reached = fleet.reaches(PROXY);
    if let Err(error) = &reached
        && outside(caller)
        && !words.contains(&"--parent")
    {
        eprintln!("fleet launch: a launch from outside the fleet has no parent, so its reports reach nobody: {}", error.user_message());
    }
    reached.is_ok()
}

pub(super) fn parent(flag: Option<String>, caller: Option<String>, proxy: bool, name: &str, role: Option<Role>) -> Option<String> {
    let is_proxy = role == Some(Role::Proxy) || name == PROXY;
    match flag.as_deref() {
        Some("none") => None,
        Some(_) => flag,
        None if is_proxy => None,
        None => caller.filter(|caller| !caller.is_empty()).or_else(|| proxy.then(|| PROXY.to_owned())),
    }
}

/// `--wait` or `--no-wait` when given; a launch from outside the fleet does not hold for admission under its fallback parent.
pub(super) fn wait(flag: Option<bool>, parent: Option<&str>, caller: Option<&str>) -> Option<bool> {
    flag.or((parent.is_none() && outside(caller)).then_some(false))
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

pub(super) fn from_proxy(fleet: &Fleet, caller: Option<&str>) -> bool {
    caller.is_some_and(|caller| caller == PROXY || fleet.role_of(caller) == Some(Role::Proxy))
}

#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
