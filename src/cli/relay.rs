use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Sent};
use crate::fleet::relay::Ssh;

use super::fleet_rebooted;

const RELAY_USAGE: &str = "Usage: north fleet relay HOST...";
const OUTBOX_USAGE: &str = "Usage: north fleet outbox take --for HOST | north fleet outbox drop ID... | north fleet outbox snapshot";
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
    match fleet.send_or_post(from.as_deref(), name, text, delivery)? {
        Sent::Here => {}
        Sent::Posted { relay: Some(relay), letter } => {
            let minutes = relay.age.as_secs() / 60;
            eprintln!("north fleet: no session named {name} here; posted {} for {}, which last relayed {minutes}m ago", letter.id, relay.host);
        }
        Sent::Posted { relay: None, letter } => eprintln!("north fleet: no session named {name} here; spooled {} for the host that has it", letter.id),
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
    let (relayed, fresh) = fleet.relay(&Ssh, &hosts)?;
    for line in relayed.lines() {
        println!("{line}");
    }
    let rebooted = fleet_rebooted::run(fleet, &["--revive"]);
    if fresh.is_empty() {
        return rebooted;
    }
    if let Err(error) = rebooted {
        eprintln!("error: {}", error.user_message());
    }
    Err(NorthError::Refused(format!("north fleet relay: newly undelivered or undropped: {}", fresh.join(", "))))
}

fn outbox(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let outbox = fleet.outbox();
    match words {
        ["take", "--for", host] => println!("{}", serde_json::to_string(&outbox.take(host)?)?),
        ["snapshot"] => println!("{}", serde_json::to_string(&fleet.snapshot()?)?),
        ["drop", ids @ ..] if !ids.is_empty() => outbox.drop(&ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>())?,
        _ => return Err(NorthError::Usage(OUTBOX_USAGE.into())),
    }
    Ok(())
}
