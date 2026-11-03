//! Who hears a launched session's reports: the calling fleet session, else the proxy for a
//! launch from outside the fleet (dispatch, timers, a bare terminal), unless `--parent` names
//! another or `none`.
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

pub(super) fn parent(flag: Option<String>, caller: Option<String>, proxy: bool, name: &str) -> Option<String> {
    match flag.as_deref() {
        Some("none") => None,
        Some(_) => flag,
        None => caller.filter(|caller| !caller.is_empty()).or_else(|| (proxy && name != PROXY).then(|| PROXY.to_owned())),
    }
}

/// `--wait` or `--no-wait` when given; a launch from outside the fleet does not hold for admission under its fallback parent.
pub(super) fn wait(flag: Option<bool>, parent: Option<&str>, caller: Option<&str>) -> Option<bool> {
    flag.or((parent.is_none() && outside(caller)).then_some(false))
}

fn outside(caller: Option<&str>) -> bool {
    caller.is_none_or(str::is_empty)
}
