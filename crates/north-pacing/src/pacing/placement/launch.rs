use std::collections::BTreeMap;
use std::path::Path;

use crate::error::NorthResult;
use crate::fleet::model::{Effort, LaunchRequest, Machine, Role};
use crate::fleet::registry::Fleet;
use crate::usage::decide::world::ModelFamily;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub local: Option<String>,
    pub pin: Option<String>,
    pub tier: Option<String>,
    pub urgent: bool,
}

impl Overrides {
    pub fn set(&mut self, flag: &str, value: &str) -> bool {
        let slot = match flag {
            "--local" => &mut self.local,
            "--pin" => &mut self.pin,
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
pub struct Placed {
    pub account: Option<String>,
    pub note: Option<String>,
    pub machine: Option<Machine>,
    pub need: Option<String>,
    /// A model the decision overrides, such as the coordinator's Opus high when no account has Fable headroom.
    pub model: Option<String>,
    pub effort: Option<Effort>,
}

#[derive(Clone, Copy)]
pub struct Launch<'a> {
    pub name: &'a str,
    pub account: &'a str,
    pub brief: Option<&'a Path>,
    /// The directory a fresh session starts in, which a machine must also have; None on revive,
    /// which resumes where the session already ran.
    pub cwd: Option<&'a Path>,
    /// The machine `--cwd HOST:PATH` or the revived record names, which placement must run the box on.
    pub machine: Option<&'a str>,
    /// A fresh launch of a lead, which staffs other hosts and so runs only where its commands have the reach to launch there.
    pub lead: bool,
    pub role: Role,
    /// The model the launch records, whose family decides which meter placement reads.
    pub model: Option<&'a str>,
    pub family: ModelFamily,
    /// The launcher is the operator, never a confined caller: see `Fleet::operator_caller`.
    pub operator: bool,
    /// The repository the launch carries in its task reference or working container, never one its brief names.
    pub repository: &'a str,
}

/// The brief placement reads: the system prompt file when there is one, then the prompt.
pub fn brief_text(system_prompt: Option<&Path>, prompt: &str) -> NorthResult<String> {
    let file = system_prompt.map(std::fs::read_to_string).transpose()?.unwrap_or_default();
    Ok(format!("{file}\n{prompt}"))
}

/// Why launch `name` placed on another machine may not start: `written`, its prompt as the caller
/// wrote it, names a path under the launching host's home, which only that host can read, other
/// than one inside `cwd`, the directory the session starts in, which the machine has. Text the
/// launch adds itself, such as another session's prior report naming its own lane, sends the worker
/// to nothing and is not read. The refusal names each path so the launch can inline the text or pass
/// it with `--system-prompt`, which the launch reads here.
pub fn home_path_refusal(name: &str, written: &str, cwd: &Path, machine: Option<&Machine>) -> Option<String> {
    let machine = machine.filter(|machine| machine.name != crate::platform::host::name())?;
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    let paths = crate::platform::home_paths::outside(written, Path::new(&home), cwd);
    (!paths.is_empty()).then(|| {
        let named: Vec<String> = paths.iter().map(|path| path.display().to_string()).collect();
        format!("{} would start on {}, but its prompt names {}, under this host's home, which {} cannot read; inline the text or pass it with --system-prompt, which the launch reads here", name, machine.name, named.join(", "), machine.name,)
    })
}

/// Live fleet sessions per machine they run on, which those machines cannot count themselves.
pub fn machine_load(fleet: &Fleet) -> NorthResult<BTreeMap<String, i64>> {
    let mut load = BTreeMap::new();
    for record in fleet.sessions()? {
        if let Some(machine) = record.machine.as_ref().filter(|_| fleet.running(&record)) {
            *load.entry(machine.name.clone()).or_default() += 1;
        }
    }
    Ok(load)
}

pub const LEAD_ON_TRUSTED: &str = "a lead staffs other hosts and its commands run on the host that runs it, so it runs on a host that places sessions on the others (the laptop or nexus)";

/// Whether the session this launch starts is a lead as its record will say: it names no parent and holds no task.
pub fn is_lead(request: &LaunchRequest) -> bool {
    request.role.map_or_else(|| request.parent.is_none() && request.tasks().is_empty() && request.name != crate::fleet::runtime::PROXY, |role| role == Role::Lead)
}

/// The family whose meter placement reads: the launch model's, else the shared Opus windows.
pub fn family_of(model: Option<&str>) -> ModelFamily {
    model.and_then(crate::usage::meter::family).unwrap_or(ModelFamily::Opus)
}

/// Whether the role is the fleet coordinator, by either spelling the role has had.
pub fn is_coordinator(role: Role) -> bool {
    matches!(role.label(), "proxy" | "coordinator")
}

/// Whether `host` is trusted: it holds the ledger or declares the hosts it `reaches`; a worker host declares none.
pub fn trusted(host: &str, here: &str) -> bool {
    let ledger = crate::threads::client::ledger_host_name();
    let reaches = super::config().ok().and_then(|cfg| cfg.get("hosts")?.get(host)?.get("reaches")?.as_array().map(|hosts| !hosts.is_empty()));
    reaches.unwrap_or(false) || ledger.as_deref() == Some(host) || (host == here && ledger.is_none())
}

/// Why a fresh lead may not run on `here`'s own registry or on the worker machine it names, None for any other launch.
pub fn lead_refusal(here: &str, launch: &Launch, trusted: impl Fn(&str) -> bool) -> Option<String> {
    if !launch.lead || launch.cwd.is_none() {
        return None;
    }
    let worker = if trusted(here) { launch.machine.filter(|machine| !trusted(machine))? } else { here };
    Some(format!("{worker} is a worker host: {LEAD_ON_TRUSTED}; launch {} from the laptop or nexus", launch.name))
}
