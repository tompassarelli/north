//! `north runtime probe list HOST`: the core-loop probes this runtime judges itself by on HOST, as one line of JSON. A
//! promotion asks the runtime it judges for this list, so a probe a commit retires stops judging that commit and the
//! ones after it, and a probe a commit adds judges it from its own promotion on. A runtime too old to print the list
//! is judged by the list of the runtime judging it.
use super::super::failure::{Failure, failed};
use super::super::layout::{self, Layout};
use super::super::probe::Shell;
use super::probes::{CoreLoop, Probe};
use super::{Live, Machine, USAGE, compiled, tail};
use crate::platform::{host, process};
use north_base::foundation::exit::NO_VERDICT;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

const ASK_LIMIT: Duration = Duration::from_secs(180);

#[derive(Serialize, Deserialize)]
struct Listed {
    probes: Vec<Entry>,
    not_applicable: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    name: String,
    script: String,
    limit_seconds: u64,
}

pub(in crate::runtime) fn print(layout: &Layout, on: &str) -> Result<(), Failure> {
    let Some((sha, _)) = super::super::own_runtime() else {
        return Err(failed(NO_VERDICT, "this north runs from no runtime, so it lists no probes of one"));
    };
    let shell = match on == host::name() {
        true => Shell::Here(on.to_owned()),
        false => Shell::Ssh(on.to_owned()),
    };
    println!("{}", printed(&compiled(on, &sha, &Live { layout, host: &shell })));
    Ok(())
}

pub(super) fn printed(plan: &CoreLoop) -> String {
    let probes = plan.probes.iter().map(|probe| Entry { name: probe.name.clone(), script: probe.script.clone(), limit_seconds: probe.limit.as_secs() }).collect();
    serde_json::to_string(&Listed { probes, not_applicable: plan.not_applicable.clone() }).unwrap_or_default()
}

pub(super) fn parsed(said: &str) -> Option<CoreLoop> {
    let line = said.lines().rev().find(|line| !line.trim().is_empty())?;
    let listed: Listed = serde_json::from_str(line).ok()?;
    let probes = listed.probes.into_iter().map(|entry| Probe { name: entry.name, script: entry.script, limit: Duration::from_secs(entry.limit_seconds) }).collect();
    Some(CoreLoop { probes, not_applicable: listed.not_applicable })
}

/// The list comes from the copy of `sha` on this host, the one a promotion built here or is about to ship, also when it judges another host.
pub(super) fn asked(layout: &Layout, on: &str, sha: &str) -> Result<CoreLoop, String> {
    let north = layout.runtime(sha).join("north");
    let short = layout::short(sha);
    let mut command = Command::new(&north);
    command.args(["runtime", "probe", "list", on]);
    let output = process::try_run_command_within(command, None, ASK_LIMIT)
        .map_err(|error| format!("{}: {error}", north.display()))?
        .ok_or_else(|| format!("{short} listed no probes within {}s", ASK_LIMIT.as_secs()))?;
    match output.status.code() {
        Some(0) => parsed(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| format!("{short} printed no probe list its judge can read")),
        Some(code) if code == i32::from(USAGE) => Err(format!("{short} is older than `north runtime probe list`")),
        _ => Err(format!("{short} could not list its probes ({}): {}", output.status, tail(&String::from_utf8_lossy(&output.stderr)))),
    }
}

pub(super) fn listed_by_live(live: &Live<'_>, on: &str, sha: &str) -> Result<CoreLoop, String> {
    match super::super::own_runtime() {
        Some((own, _)) if own == sha => Ok(compiled(on, sha, live)),
        _ => asked(live.layout, on, sha),
    }
}

/// What a reviewer reads before a promotion: the probes `judged` adds to and drops from the list of the runtime live on the host.
pub(super) fn against_live(on: &str, sha: &str, judged: &CoreLoop, live: Option<&str>, machine: &impl Machine) -> String {
    let Some(live) = live.filter(|live| *live != sha) else {
        return "no other runtime is live there to compare its probe list with".to_owned();
    };
    let kept = layout::short(live);
    let list = match machine.listed_by(on, live) {
        Ok(list) => list,
        Err(why) => return format!("its probe list is not compared with that of {kept}, the runtime live there: {why}"),
    };
    let only = |from: &CoreLoop, other: &CoreLoop| -> Vec<String> { from.probes.iter().filter(|probe| !other.probes.iter().any(|held| held.name == probe.name)).map(|probe| format!("`{}`", probe.name)).collect() };
    let (added, removed) = (only(judged, &list), only(&list, judged));
    if added.is_empty() && removed.is_empty() {
        return format!("judged by the same {} probes as {kept}, the runtime live there", judged.probes.len());
    }
    let named = |names: Vec<String>| if names.is_empty() { "none".to_owned() } else { names.join(", ") };
    format!("against {kept}, the runtime live there: probes added: {}; probes removed: {}", named(added), named(removed))
}
