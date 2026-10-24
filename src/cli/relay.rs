use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Sent};
use crate::fleet::relay::Ssh;

const RELAY_USAGE: &str = "Usage: north fleet relay HOST...";
const OUTBOX_USAGE: &str = "Usage: north fleet outbox take --for HOST | north fleet outbox drop ID...";
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
        Sent::Posted { relay, letter } => {
            let minutes = relay.age.as_secs() / 60;
            eprintln!("north fleet: no session named {name} here; posted {} for {}, which last relayed {minutes}m ago", letter.id, relay.host);
        }
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
    if fresh.is_empty() {
        return Ok(());
    }
    Err(NorthError::Refused(format!("north fleet relay: newly undelivered or undropped: {}", fresh.join(", "))))
}

fn outbox(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let outbox = fleet.outbox();
    match words {
        ["take", "--for", host] => println!("{}", serde_json::to_string(&outbox.take(host)?)?),
        ["drop", ids @ ..] if !ids.is_empty() => outbox.drop(&ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>())?,
        _ => return Err(NorthError::Usage(OUTBOX_USAGE.into())),
    }
    Ok(())
}
