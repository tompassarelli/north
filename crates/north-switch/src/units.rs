//! A host's systemd user units as `north fleet off` and `on` see them: which are active, which are transient
//! (gone once stopped, so nothing can start them again), and the stop, kill, start, disable and enable that act on them.

use crate::host::{Host, quote};

const ACTIVE: [&str; 4] = ["active", "activating", "reloading", "refreshing"];
const TRIGGERS: [&str; 3] = [".timer", ".path", ".socket"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub name: String,
    pub transient: bool,
    pub enabled: bool,
}

pub struct Listing {
    pub hostname: String,
    pub active: Vec<String>,
}

pub fn is_trigger(unit: &str) -> bool {
    TRIGGERS.iter().any(|suffix| unit.ends_with(suffix))
}

pub fn triggered_by<'a>(service: &str, units: &'a [Unit]) -> Option<&'a Unit> {
    let stem = service.strip_suffix(".service")?;
    units.iter().find(|unit| unit.name.rsplit_once('.').is_some_and(|(name, kind)| name == stem && kind != "service" && is_trigger(&unit.name)))
}

pub fn list(host: &Host) -> Result<Listing, String> {
    let text = host.run("cat /proc/sys/kernel/hostname && systemctl --user list-units --all --plain --no-legend --full --type=service,scope,timer,path,socket")?;
    let mut lines = text.lines();
    let hostname = lines.next().unwrap_or_default().trim().to_owned();
    let active = lines
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (name, _load, state) = (fields.next()?, fields.next()?, fields.next()?);
            ACTIVE.contains(&state).then(|| name.to_owned())
        })
        .collect();
    Ok(Listing { hostname, active })
}

pub fn described(host: &Host, names: &[String]) -> Result<Vec<Unit>, String> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let shown = host.run(&format!("systemctl --user show --property=Id,Transient,UnitFileState -- {}", words(names)))?;
    Ok(names
        .iter()
        .map(|name| {
            let block = shown.split("\n\n").find(|block| block.lines().any(|line| line.strip_prefix("Id=") == Some(name)));
            let transient = block.is_some_and(|block| block.lines().any(|line| line == "Transient=yes"));
            let enabled = block.is_some_and(|block| block.lines().any(|line| line == "UnitFileState=enabled"));
            Unit { name: name.clone(), transient, enabled }
        })
        .collect())
}

pub fn stop(host: &Host, names: &[String]) -> Result<(), String> {
    act(host, "stop", names)
}

pub fn stop_now(host: &Host, names: &[String]) -> Result<(), String> {
    act(host, "stop --no-block", names)
}

pub fn kill(host: &Host, names: &[String]) -> Result<(), String> {
    act(host, "kill --signal=SIGKILL", names)
}

pub fn disable(host: &Host, names: &[String]) -> Result<(), String> {
    act(host, "disable", names)
}

pub fn enable(host: &Host, names: &[String]) -> Result<(), String> {
    act(host, "enable", names)
}

pub fn start(host: &Host, names: &[String]) -> Result<(), String> {
    act(host, "start", names)
}

fn act(host: &Host, verb: &str, names: &[String]) -> Result<(), String> {
    match names.is_empty() {
        true => Ok(()),
        false => host.run(&format!("systemctl --user {verb} -- {}", words(names))).map(drop),
    }
}

fn words(names: &[String]) -> String {
    names.iter().map(|name| quote(name)).collect::<Vec<_>>().join(" ")
}
