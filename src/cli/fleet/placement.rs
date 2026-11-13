//! The placement gate in front of `north fleet launch` and `revive`: a Claude box starts only where
//! placement puts it, or with the override reason recorded on the session.
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Machine, Role};
use crate::fleet::registry::Fleet;
pub(super) use crate::pacing::placement::launch::{Launch, Overrides, Placed, brief_text, machine_load};
pub(super) use crate::pacing::placement::place::check;
use crate::pacing::placement::place::Placer;
pub(super) use crate::pacing::placement::refusal::Refusal;
use crate::spend::Starter;

/// Whether this launch comes from the operator, never a confined caller: outside every worker sandbox, not a queue replay, whose original caller is unproven, not a lead's spooled launch, which the fleet tick runs outside every session scope for a sandboxed lead, and judged by the launcher's own cgroup (`Fleet::operator_caller`).
pub(super) fn operator_launcher(fleet: &Fleet, caller: Option<&str>, queued: Option<&str>) -> bool {
    let Ok(cgroup) = std::fs::read_to_string("/proc/self/cgroup") else { return false };
    let spooled = std::env::var_os(crate::fleet::runtime::SPOOLED_ENV).is_some();
    operator_launch(queued.is_some(), spooled, north_runtime::worker_sandbox::confined(), || fleet.operator_caller(caller, &cgroup))
}

fn operator_launch(queued: bool, spooled: bool, confined: bool, outside_every_scope: impl FnOnce() -> bool) -> bool {
    !queued && !spooled && !confined && outside_every_scope()
}

pub(super) fn placed(fleet: &Fleet, request: &LaunchRequest, written: &str, needs: &[String], overrides: &Overrides, report: bool, operator: bool) -> NorthResult<Result<Placed, Refusal>> {
    Placer { fleet, spend_now: crate::dispatch::spend_now, harness_cap }.placed(request, written, needs, overrides, report, operator)
}

fn harness_cap(fleet: &Fleet, request: &LaunchRequest) -> Result<(), String> {
    use crate::dispatch::deepseek::{PROVIDER, Settings, cap};
    if request.provider.label() != PROVIDER {
        return Ok(());
    }
    let sessions = fleet.sessions().map_err(|error| error.to_string())?;
    cap::admit(cap::running(sessions.iter().filter(|record| fleet.running(record)).map(|record| record.provider.label())), Settings::read().slot_cap)
}

/// Whether the spend rule admits one more session on the request's own account: what a tick's hand-over checks before it starts a session elsewhere.
pub(super) fn spend_admits(fleet: &Fleet, request: &LaunchRequest) -> NorthResult<()> {
    let starter = if request.role == Some(Role::Proxy) { Starter::Reserved } else { Starter::Worker };
    let view = crate::dispatch::spend_now(&fleet.roster()).map_err(|why| NorthError::Saturated(format!("spend view unread: {why}")))?;
    let account = format!("{}:{}", request.provider.label(), request.account);
    view.admit(&account, starter).map_err(|refusal| NorthError::Saturated(refusal.to_string()))
}

/// Where a fresh session starts: `HOST:/PATH` names a `[hosts.HOST]` machine and a directory there,
/// anything else is a directory here; none is the current directory.
pub(super) fn working_directory(cwd: Option<PathBuf>) -> NorthResult<(PathBuf, Option<Machine>)> {
    let Some(cwd) = cwd else {
        return Ok((std::env::current_dir()?, None));
    };
    let named = cwd.to_str().and_then(|text| text.split_once(':')).filter(|(host, path)| !host.contains('/') && path.starts_with('/'));
    let Some((host, path)) = named else {
        return Ok((std::fs::canonicalize(cwd)?, None));
    };
    let machine = crate::pacing::placement::machine(host).ok_or_else(|| NorthError::Usage(format!("--cwd {host}:{path} names no [hosts.{host}] with an ssh destination")))?;
    Ok((PathBuf::from(path), Some(machine)))
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;
