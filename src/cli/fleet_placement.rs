//! The placement gate in front of `north fleet launch` and `revive`: a Claude box starts only where
//! placement puts it, or with the override reason recorded on the session.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Machine;
use crate::fleet::registry::Fleet;
use crate::fleet::roster::Roster;
use crate::fleet::runtime::binary;
use crate::ops::dispatch::{cloud_autoland, cloud_branch};
use crate::ops::pacing::placement::gate::{self, Decision, Target};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Overrides {
    pub local: Option<String>,
    pub pin: Option<String>,
    /// Why a `--task` launch runs beside the session or dispatch launch that already holds its item.
    pub duplicate: Option<String>,
    pub tier: Option<String>,
}

impl Overrides {
    pub fn set(&mut self, flag: &str, value: &str) -> bool {
        let slot = match flag {
            "--local" => &mut self.local,
            "--pin" => &mut self.pin,
            "--force-dup" => &mut self.duplicate,
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
    pub note: Option<String>,
    pub machine: Option<Machine>,
    pub need: Option<String>,
}

pub(super) struct Launch<'a> {
    pub name: &'a str,
    pub account: &'a str,
    pub brief: Option<&'a Path>,
    /// The directory a fresh session starts in, which a machine must also have; None on revive,
    /// which resumes where the session already ran.
    pub cwd: Option<&'a Path>,
    /// The machine `--cwd HOST:PATH` or the revived record names, which placement must run the box on.
    pub machine: Option<&'a str>,
}

/// The brief placement reads: the system prompt file when there is one, then the prompt.
pub(super) fn brief_text(system_prompt: Option<&Path>, prompt: &str) -> NorthResult<String> {
    let file = system_prompt.map(std::fs::read_to_string).transpose()?.unwrap_or_default();
    Ok(format!("{file}\n{prompt}"))
}

/// Runs live placement, prints one verdict line per account and machine, and returns where to run.
/// `fleet_load` counts the live fleet sessions this machine runs on each other machine.
pub(super) fn check(launch: &Launch, text: &str, overrides: &Overrides, fleet_load: &BTreeMap<String, i64>, roster: &Roster) -> NorthResult<Placed> {
    let mut decision = gate::live(text, fleet_load, roster, launch.machine).map_err(|error| NorthError::Configuration(format!("placement: {error}")))?;
    let absent = match (&decision.machine, launch.cwd) {
        (Some(machine), Some(cwd)) if !has_directory(&machine.ssh, cwd) => Some(format!("{} has no {}, so here", machine.name, cwd.display())),
        _ => None,
    };
    if absent.is_some() {
        decision.stay_here(absent);
    }
    decision.hold_here();
    eprintln!("placement: {}", gate::summary(&decision));
    for line in decision.verdicts.iter().map(|verdict| &verdict.line).chain(&decision.machine_lines) {
        eprintln!("  {line}");
    }
    admit(&decision, launch, text, overrides).map_err(NorthError::Refused)
}

/// Ok(where) when the launch may start, Err(refusal with the way forward) otherwise. `--local` keeps
/// the box here instead of the cloud or another machine.
pub(super) fn admit(decision: &Decision, launch: &Launch, text: &str, overrides: &Overrides) -> Result<Placed, String> {
    let mut notes = Vec::new();
    let elsewhere = matches!(decision.target, Target::Cloud(_)) || decision.machine.is_some();
    match (&decision.target, &overrides.local) {
        (Target::Unserved(why), _) => return Err(format!("{why}; list it in a holding host's [hosts.NAME] serves")),
        (Target::Cloud(credit), None) => return Err(cloud_command(launch, credit, &decision.surface, text)),
        (_, Some(reason)) if elsewhere || decision.here_saturated.is_some() => notes.push(format!("local: {reason}")),
        _ => {}
    }
    let machine = match (&decision.machine, &overrides.local) {
        (Some(chosen), None) if !chosen.accounts.iter().any(|label| label == launch.account) => return Err(not_signed_in(decision, chosen, launch.account)),
        (Some(chosen), None) => Some(Machine { name: chosen.name.clone(), ssh: chosen.ssh.clone() }),
        _ => None,
    };
    if let Some(out) = exhausted_refusal(decision, launch.account) {
        return Err(out);
    }
    let saturated = decision.saturated.as_ref().filter(|_| machine.is_some()).map(|why| format!("{why}; launch it elsewhere, or keep it on that machine with --pin \"<reason>\", or here with --local \"<reason>\""));
    let hot = decision.here_saturated.as_ref().filter(|_| machine.is_none() && overrides.local.is_none()).map(|why| format!("{why}; launch from another host, or keep it here with --local \"<reason>\" or --pin \"<reason>\""));
    if let Some(refusal) = saturated.or(hot).or_else(|| account_refusal(decision, launch.account)) {
        let Some(reason) = &overrides.pin else {
            return Err(refusal);
        };
        notes.push(format!("pin: {reason}"));
    }
    if let Some(reason) = &overrides.tier {
        notes.push(format!("tier: {reason}"));
    }
    Ok(Placed { note: (!notes.is_empty()).then(|| notes.join("; ")), machine, need: gate::capacity_need(text) })
}

fn not_signed_in(decision: &Decision, machine: &gate::Machine, account: &str) -> String {
    let there = if machine.accounts.is_empty() { "none".to_owned() } else { machine.accounts.join(", ") };
    let instead = match &decision.target {
        Target::Account(other) if other.trim_start_matches("claude:") != account => format!("launch with --account {}, or ", other.trim_start_matches("claude:")),
        _ => String::new(),
    };
    format!("{} is not signed in to claude:{account} (signed in there: {there}); {instead}keep the box here with --local \"<reason>\"", machine.name)
}

/// An account that is out cannot run the box at all, so no `--pin`, recorded or given, keeps it.
fn exhausted_refusal(decision: &Decision, account: &str) -> Option<String> {
    let chosen = decision.verdict(&format!("claude:{account}")).filter(|verdict| verdict.exhausted)?;
    let instead = match picked(decision) {
        Some(better) => format!("placement picks {} ({}); launch with --account {}", better.account, better.line, better.account.trim_start_matches("claude:")),
        None => "no Claude account is available; wait for a window to reset".to_owned(),
    };
    Some(format!("{}; {instead}", chosen.line))
}

fn picked(decision: &Decision) -> Option<&gate::Verdict> {
    match &decision.target {
        Target::Account(placed) => decision.verdict(placed),
        _ => decision.best_here(),
    }
}

fn account_refusal(decision: &Decision, account: &str) -> Option<String> {
    let name = format!("claude:{account}");
    let chosen = decision.verdict(&name);
    let best = picked(decision);
    if chosen.is_none() && best.is_none() {
        return None;
    }
    let fits = chosen.is_some_and(|v| v.available) && best.is_none_or(|b| chosen.is_some_and(|v| b.rank.order(&v.rank).is_le()));
    if fits {
        return None;
    }
    let why = chosen.map_or_else(|| format!("{name} has no live quota sample"), |v| v.line.clone());
    let instead = match best {
        Some(better) => format!("placement picks {} ({}); launch with --account {}", better.account, better.line, better.account.trim_start_matches("claude:")),
        None => "no Claude account is available".to_owned(),
    };
    Some(format!("{why}; {instead}, or keep {name} with --account {account} --pin \"<reason>\""))
}

fn has_directory(ssh: &str, directory: &Path) -> bool {
    let quoted = format!("'{}'", directory.to_string_lossy().replace('\'', r"'\''"));
    Command::new(binary("ssh"))
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", "test", "-d", &quoted])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn cloud_command(launch: &Launch, credit: &str, surface: &str, text: &str) -> String {
    let brief = launch.brief.map_or_else(|| "BRIEF.md".to_owned(), |path| path.display().to_string());
    let repo = gate::repository(text);
    let branch = cloud_branch(launch.name);
    let landing = if cloud_autoland(&repo) {
        format!("Autoland lands {branch} once its checks pass, so no courier runs.")
    } else {
        format!(
            "Land it from gex45 or the laptop, never Nexus: once git ls-remote origin {branch} shows it, fetch it into a worktree, rerun the checks, land it with safe-push --to main and delete it with git push origin --delete {branch}. \
             If the session reports its push refused, collect its handback with north cloud wait <id> OUT.jsonl on Nexus and decode it as cloud.md says."
        )
    };
    format!(
        "placement sends this box to anthropic-cloud ({credit}; {surface}). Run it on Nexus instead (harness/skills/workers/references/cloud.md):\n  \
         ssh nexus 'mkdir -p /tmp/cloud && cat > /tmp/cloud/{name}.md' < {brief}\n  \
         ssh nexus 'north cloud launch {repo} {name} /tmp/cloud/{name}.md'\n\
         It launches claude --cloud from /tmp/cloud/src/{repo}, adds the cloud safety line and the steps that push {branch}, and prints the session_... ID; record it in the box's thread.\n\
         {landing}\n\
         To run it here anyway, relaunch with --local \"<reason>\".",
        name = launch.name,
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

pub(super) fn fleet_load() -> NorthResult<BTreeMap<String, i64>> {
    machine_load(&super::fleet::open_fleet()?)
}

#[cfg(test)]
#[path = "fleet_placement_tests.rs"]
mod tests;

/// Where a fresh session starts: `HOST:/PATH` names a `[hosts.HOST]` machine and a directory there,
/// anything else is a directory here; none is the current directory.
pub(super) fn working_directory(cwd: Option<PathBuf>) -> NorthResult<(PathBuf, Option<Machine>)> {
    let Some(cwd) = cwd else { return Ok((std::env::current_dir()?, None)) };
    let named = cwd.to_str().and_then(|text| text.split_once(':')).filter(|(host, path)| !host.contains('/') && path.starts_with('/'));
    let Some((host, path)) = named else { return Ok((std::fs::canonicalize(cwd)?, None)) };
    let machine = crate::ops::pacing::placement::machine(host).ok_or_else(|| NorthError::Usage(format!("--cwd {host}:{path} names no [hosts.{host}] with an ssh destination")))?;
    Ok((PathBuf::from(path), Some(machine)))
}
