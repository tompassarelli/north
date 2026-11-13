use crate::error::{NorthError, NorthResult};
use crate::fleet::fold::Condition;
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Sent};
use crate::fleet::sender::Sender;

const SEND_USAGE: &str = "Usage: north fleet send [--now] [--from service:UNIT [--key CONDITION [--cleared]]] NAME TEXT...";

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let (flags, rest) = leading_flags(words)?;
    let condition = condition(&flags)?;
    match rest {
        [name, text @ ..] if !text.is_empty() => send(fleet, flags.from, name, &text.join(" "), condition.as_ref(), if flags.now { Delivery::Now } else { Delivery::Queued }),
        _ => Err(NorthError::Usage(SEND_USAGE.into())),
    }
}

pub(super) struct Flags<'a> {
    pub from: Option<&'a str>,
    pub now: bool,
    pub key: Option<&'a str>,
    pub cleared: bool,
}

pub(super) fn leading_flags<'a, 'w>(mut words: &'w [&'a str]) -> NorthResult<(Flags<'a>, &'w [&'a str])> {
    let mut flags = Flags { from: None, now: false, key: None, cleared: false };
    loop {
        match words {
            ["--now", rest @ ..] => (flags.now, words) = (true, rest),
            ["--cleared", rest @ ..] => (flags.cleared, words) = (true, rest),
            ["--from", from, rest @ ..] => (flags.from, words) = (Some(*from), rest),
            ["--key", key, rest @ ..] => (flags.key, words) = (Some(*key), rest),
            ["--from" | "--key"] => return Err(NorthError::Usage(SEND_USAGE.into())),
            _ => return Ok((flags, words)),
        }
    }
}

fn condition(flags: &Flags) -> NorthResult<Option<Condition>> {
    match (flags.key, flags.from.is_some_and(|from| from.starts_with("service:"))) {
        (Some(key), true) if !key.is_empty() => Ok(Some(Condition { key: key.to_owned(), cleared: flags.cleared })),
        (None, _) if !flags.cleared => Ok(None),
        _ => Err(NorthError::Usage(format!("--key names a service's condition and --cleared ends it, so both need --from service:UNIT and --cleared needs --key; {SEND_USAGE}"))),
    }
}

fn send(fleet: &Fleet, from: Option<&str>, name: &str, text: &str, condition: Option<&Condition>, delivery: Delivery) -> NorthResult<()> {
    let from = signer(fleet, from, name)?;
    match fleet.send_condition(&from, name, text, condition, delivery)? {
        Sent::Spooled { id, host } => eprintln!("north fleet: {name} runs on {host}; spooled {id} for the fleet tick to deliver"),
        Sent::Queued { id } => eprintln!("north fleet: spooled {id}; this host's fleet tick hands it to {name}"),
        Sent::Retired { heir } => eprintln!("north fleet: {name} is retired; the message went to {heir}"),
        Sent::Unreached => eprintln!("north fleet: no live or parked session is named {name}; the message went to the proxy"),
        Sent::Held => eprintln!("north fleet: held for `north work`: {name} already has this alarm or condition open, or it cleared"),
        Sent::Flooded { line } => {
            eprintln!("north fleet: held for `north work`: {from} is past the proxy's page ceiling, so the proxy was told once");
            super::fleet::page_flood::record(&from, &line);
        }
        Sent::HandedIn => {}
    }
    Ok(())
}

pub(super) fn signer(fleet: &Fleet, from: Option<&str>, to: &str) -> NorthResult<Sender> {
    let caller = fleet.caller();
    let sender = Sender::declared(from, caller)?;
    if let Some(rival) = fleet.rival(&sender, to) {
        eprintln!("{rival}");
    }
    Ok(sender)
}
