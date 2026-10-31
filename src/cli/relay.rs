use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Sent};
use crate::fleet::relay::Ssh;

use super::fleet_rebooted;

const RELAY_USAGE: &str = "Usage: north fleet relay HOST...";
const OUTBOX_USAGE: &str = "Usage: north fleet outbox snapshot";
const SEND_USAGE: &str = "Usage: north fleet send [--now] NAME TEXT...";

pub fn run(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    match (verb, words) {
        ("send", ["--now", name, text @ ..]) if !text.is_empty() => send(fleet, name, &text.join(" "), Delivery::Now),
        ("send", [name, text @ ..]) if !text.is_empty() => send(fleet, name, &text.join(" "), Delivery::Queued),
        ("send", _) => Err(NorthError::Usage(SEND_USAGE.into())),
        ("relay", hosts) => relay(fleet, hosts),
        _ => outbox(fleet, words),
    }
}

fn send(fleet: &Fleet, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
    let from = signer(fleet, name);
    if let Sent::Spooled { id, host } = fleet.send_message(from.as_deref(), name, text, delivery)? {
        eprintln!("north fleet: {name} runs on {host}; spooled {id} for the fleet tick to deliver");
    }
    Ok(())
}

pub(super) fn signer(fleet: &Fleet, to: &str) -> Option<String> {
    let from = fleet.caller()?;
    if let Some(rival) = fleet.rival(&from, to) {
        eprintln!("{rival}");
    }
    Some(from)
}

fn relay(fleet: &Fleet, hosts: &[&str]) -> NorthResult<()> {
    if hosts.is_empty() || hosts.iter().any(|host| host.starts_with('-')) {
        return Err(NorthError::Usage(RELAY_USAGE.into()));
    }
    let hosts: Vec<String> = hosts.iter().map(|host| (*host).to_owned()).collect();
    for line in fleet.relay(&Ssh, &hosts)?.lines() {
        println!("{line}");
    }
    fleet_rebooted::run(fleet, &["--revive"])
}

fn outbox(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["snapshot"] => println!("{}", serde_json::to_string(&fleet.snapshot()?)?),
        _ => return Err(NorthError::Usage(OUTBOX_USAGE.into())),
    }
    Ok(())
}
