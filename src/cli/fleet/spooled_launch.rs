use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::{self, Fleet};
use crate::fleet::runtime::SESSION_ENV;

use super::launch::launch_parts;
use super::lineage;

/// `north fleet launch`: `direct` starts it here, or it is spooled when this process may not write the fleet's session records.
pub(super) fn or_spool(fleet: &Fleet, words: &[&str], direct: fn(&Fleet, &[&str]) -> NorthResult<()>) -> NorthResult<()> {
    if fleet.writes_records() {
        return direct(fleet, words);
    }
    run(fleet, std::env::var(SESSION_ENV).ok(), words)
}

/// A launch from a process that may not write the fleet's session records, such as a lead in the worker sandbox: its words go to the session's spool, and this host's fleet tick runs them as that session, so every admission of a direct launch judges them there.
pub(super) fn run(fleet: &Fleet, caller: Option<String>, words: &[&str]) -> NorthResult<()> {
    let caller = caller.filter(|name| !name.is_empty()).ok_or_else(|| NorthError::Usage(format!("north fleet launch: this process may not write the fleet's session records and ${SESSION_ENV} is unset, so no session's spool can carry the launch to this host's fleet tick")))?;
    let proxy = lineage::proxy_reachable(fleet, Some(&caller), words);
    let parts = launch_parts(&fleet.providers(), words, Some(caller.clone()), proxy)?;
    let name = parts.request.name;
    let words: Vec<String> = ["--name", name.as_str()].into_iter().chain(words.iter().copied()).map(str::to_owned).collect();
    if let Some(flag) = registry::refused_flag(&words) {
        return Err(NorthError::Usage(format!("north fleet launch {name}: a launch from the worker sandbox cannot carry {flag}, since the host would use or judge it outside the sandbox; put what it names in the prompt, or ask your lead to launch it")));
    }
    let row = fleet.spool_launch(&caller, &name, &words)?;
    println!("spooled LAUNCH row {}: {name} starts once this host's fleet tick applies it, under the checks a direct launch runs", row.id);
    Ok(())
}
