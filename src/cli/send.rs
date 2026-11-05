use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Sent};
use crate::fleet::sender::Sender;

const SEND_USAGE: &str = "Usage: north fleet send [--now] [--from service:UNIT] NAME TEXT...";

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let (flags, rest) = leading_flags(words)?;
    match rest {
        [name, text @ ..] if !text.is_empty() => send(fleet, flags.from, name, &text.join(" "), if flags.now { Delivery::Now } else { Delivery::Queued }),
        _ => Err(NorthError::Usage(SEND_USAGE.into())),
    }
}

pub(super) struct Flags<'a> {
    pub from: Option<&'a str>,
    pub now: bool,
}

pub(super) fn leading_flags<'a, 'w>(mut words: &'w [&'a str]) -> NorthResult<(Flags<'a>, &'w [&'a str])> {
    let mut flags = Flags { from: None, now: false };
    loop {
        match words {
            ["--now", rest @ ..] => (flags.now, words) = (true, rest),
            ["--from", from, rest @ ..] => (flags.from, words) = (Some(*from), rest),
            ["--from"] => return Err(NorthError::Usage(SEND_USAGE.into())),
            _ => return Ok((flags, words)),
        }
    }
}

fn send(fleet: &Fleet, from: Option<&str>, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
    let from = signer(fleet, from, name)?;
    if let Sent::Spooled { id, host } = fleet.send_message(&from, name, text, delivery)? {
        eprintln!("north fleet: {name} runs on {host}; spooled {id} for the fleet tick to deliver");
    }
    Ok(())
}

pub(super) fn signer(fleet: &Fleet, from: Option<&str>, to: &str) -> NorthResult<Sender> {
    let sender = Sender::declared(from, fleet.caller())?;
    if let Some(rival) = fleet.rival(&sender, to) {
        eprintln!("{rival}");
    }
    Ok(sender)
}
