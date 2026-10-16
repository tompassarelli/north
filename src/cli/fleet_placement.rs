//! The placement gate in front of `north fleet launch` and `revive`: a Claude box starts only where
//! placement puts it, or with the override reason recorded on the session.
use std::path::Path;

use crate::error::{NorthError, NorthResult};
use crate::ops::pacing::placement::gate::{self, Decision, Target};

const SAFETY_LINE: &str = "Run only build, test, git and file commands inside your clone and /tmp. Never kill, pkill or signal a process, never use sudo, and use no network beyond git clone, git fetch and package installs. If a command is blocked or would need approval, do not retry it or work around it; stop and report `Blocked:` with the command.";
const HANDBACK: &str = "git format-patch origin/main..HEAD --stdout | gzip -9 | base64 -w 76 > /tmp/handback.b64, then one Bash call per 300-line part i of n: echo BEGIN-HANDBACK part i/n; sed -n <first>,<last>p /tmp/handback.b64; echo END-HANDBACK part i/n";

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Overrides {
    pub local: Option<String>,
    pub pin: Option<String>,
}

impl Overrides {
    /// Takes `--local REASON` or `--pin REASON`; false for any other flag.
    pub fn set(&mut self, flag: &str, value: &str) -> bool {
        let slot = match flag {
            "--local" => &mut self.local,
            "--pin" => &mut self.pin,
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
                _ => {}
            }
        }
        self
    }
}

pub(super) struct Launch<'a> {
    pub name: &'a str,
    pub account: &'a str,
    pub brief: Option<&'a Path>,
}

/// The brief placement reads: the system prompt file when there is one, then the prompt.
pub(super) fn brief_text(system_prompt: Option<&Path>, prompt: &str) -> NorthResult<String> {
    let file = system_prompt.map(std::fs::read_to_string).transpose()?.unwrap_or_default();
    Ok(format!("{file}\n{prompt}"))
}

/// Runs live placement, prints one verdict line per account, and returns the note to record.
pub(super) fn check(launch: &Launch, text: &str, overrides: &Overrides) -> NorthResult<Option<String>> {
    let decision = gate::live(text).map_err(|error| NorthError::Configuration(format!("placement: {error}")))?;
    eprintln!("placement: {}", gate::summary(&decision));
    for verdict in &decision.verdicts {
        eprintln!("  {}", verdict.line);
    }
    admit(&decision, launch, text, overrides).map_err(NorthError::Refused)
}

/// Ok(note) when the launch may start, Err(refusal with the way forward) otherwise.
pub(super) fn admit(decision: &Decision, launch: &Launch, text: &str, overrides: &Overrides) -> Result<Option<String>, String> {
    let mut notes = Vec::new();
    if let Target::Cloud(credit) = &decision.target {
        let Some(reason) = &overrides.local else {
            return Err(cloud_command(launch, credit, &decision.surface, text));
        };
        notes.push(format!("local: {reason}"));
    }
    if let Some(refusal) = account_refusal(decision, launch.account) {
        let Some(reason) = &overrides.pin else {
            return Err(refusal);
        };
        notes.push(format!("pin: {reason}"));
    }
    Ok((!notes.is_empty()).then(|| notes.join("; ")))
}

fn account_refusal(decision: &Decision, account: &str) -> Option<String> {
    let name = format!("claude:{account}");
    let chosen = decision.verdict(&name);
    let best = decision.best_account();
    if chosen.is_none() && best.is_none() {
        return None;
    }
    let fits = chosen.is_some_and(|v| v.available) && best.is_none_or(|b| b.headroom <= chosen.map_or(0.0, |v| v.headroom));
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

fn cloud_command(launch: &Launch, credit: &str, surface: &str, text: &str) -> String {
    let brief = launch.brief.map_or_else(|| "BRIEF.md".to_owned(), |path| path.display().to_string());
    let safety = if text.contains(SAFETY_LINE) {
        "The brief carries the cloud safety line.".to_owned()
    } else {
        format!("Add the cloud safety line to the brief first: \"{SAFETY_LINE}\"")
    };
    format!(
        "placement sends this box to anthropic-cloud ({credit}; {surface}). Run it on Nexus instead (harness/skills/workers/references/cloud.md):\n  \
         ssh nexus 'mkdir -p /tmp/cloud && cat > /tmp/cloud/{name}.md' < {brief}\n  \
         ssh -tt nexus 'cd /tmp/cloud && claude --cloud \"$(cat {name}.md)\"'\n\
         {safety}\nThe brief ends with the handback: {HANDBACK}.\n\
         Record the printed session_... ID in the box's thread and collect it with cloud-sessions.sh list on Nexus.\n\
         To run it here anyway, relaunch with --local \"<reason>\".",
        name = launch.name,
    )
}

#[cfg(test)]
#[path = "fleet_placement_tests.rs"]
mod tests;
