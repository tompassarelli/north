use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Sent};

const SEND_USAGE: &str = "Usage: north fleet send [--now] NAME TEXT...";

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["--now", name, text @ ..] if !text.is_empty() => send(fleet, name, &text.join(" "), Delivery::Now),
        [name, text @ ..] if !text.is_empty() => send(fleet, name, &text.join(" "), Delivery::Queued),
        _ => Err(NorthError::Usage(SEND_USAGE.into())),
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
