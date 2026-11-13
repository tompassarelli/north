use super::cloud::{autoland as cloud_autoland, branch as cloud_branch};
use super::gate::{self, CloudLane, Decision, Target};
use super::launch::{Launch, Overrides, Placed, is_coordinator};
use super::refusal::Refusal;
use crate::fleet::model::{Effort, Machine};
use crate::usage::decide::world::ModelFamily;

pub fn settle(decision: &Decision, launch: &Launch, text: &str, overrides: &Overrides) -> Result<Placed, Refusal> {
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

pub fn pick(decision: &Decision, overrides: &Overrides) -> Result<String, Refusal> {
    match (&decision.target, &overrides.local) {
        (Target::Held(why), _) if decision.held_for_room() => Err(Refusal::Saturated(format!("held: {why}"))),
        (Target::Held(why), _) => Err(Refusal::Final(format!("held: {why}"))),
        (Target::Unserved(_), _) | (Target::Cloud(_), None) => Ok(String::new()),
        _ => picked(decision).map(|verdict| verdict.account.trim_start_matches("claude:").to_owned()).ok_or_else(|| Refusal::Final("no Claude account is available; wait for a window to reset".to_owned())),
    }
}

/// A fresh launch without `--pin` or `--local` falls through a saturated machine it named; a revive stays where its conversation is.
pub fn fall_through(decision: &mut Decision, launch: &Launch, overrides: &Overrides) {
    if launch.cwd.is_some() && overrides.pin.is_none() && overrides.local.is_none() {
        decision.fall_through(launch.account);
    }
}

/// Ok(where) when the launch may start, Err(refusal with the way forward) otherwise. `--local` keeps
/// the box here instead of the cloud or another machine.
pub fn admit(decision: &Decision, launch: &Launch, text: &str, overrides: &Overrides) -> Result<Placed, Refusal> {
    let mut notes = Vec::new();
    let elsewhere = matches!(decision.target, Target::Cloud(_)) || decision.machine.is_some();
    match (&decision.target, &overrides.local) {
        (Target::Unserved(why), _) => {
            return Err(Refusal::Final(format!("{why}; list it in a holding host's [hosts.NAME] serves")));
        }
        (Target::Cloud(lane), None) => {
            return Err(Refusal::Final(cloud_command(launch, lane, &decision.here, &decision.surface, text)));
        }
        (_, Some(reason)) if elsewhere || decision.here_saturated.is_some() => notes.push(format!("local: {reason}")),
        _ => {}
    }
    let machine = match (&decision.machine, &overrides.local) {
        (Some(chosen), None) if !chosen.accounts.iter().any(|label| label == launch.account) => {
            return Err(Refusal::Final(not_signed_in(decision, chosen, launch.account)));
        }
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

pub fn picked(decision: &Decision) -> Option<&gate::Verdict> {
    match &decision.target {
        Target::Account(placed) => decision.verdict(placed),
        _ => decision.best_here(),
    }
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
    let (copy, start) = if lane.host == here { (format!("mkdir -p /tmp/cloud && cp {brief} /tmp/cloud/{name}.md"), format!("{login} north cloud launch {repo} {name} /tmp/cloud/{name}.md")) } else { (format!("ssh {host} 'mkdir -p /tmp/cloud && cat > /tmp/cloud/{name}.md' < {brief}", host = lane.host), format!("ssh {host} '{login} north cloud launch {repo} {name} /tmp/cloud/{name}.md'", host = lane.host)) };
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
