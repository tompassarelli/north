use std::io::{BufRead, Write};

use north_base::foundation::offload::{HOLD_ENV, RUN_SECONDS_CEILING};

use crate::capacity::carry;

/// The line the host prints on its standard output once a hold has its slot, followed by the slot's name.
pub(super) const HOLDING: &str = "offload: holding ";
/// The command a hold stands for in the host's slot line and in-flight marker, so every hold of one head shares one check name there.
pub const HOLD_COMMAND: &str = "north-offload-hold";
/// The longest a hold keeps its slot: two of the longest land checks, since it runs a hand-in's checks one after another or side by side.
pub const HOLD_CEILING_SECONDS: u64 = 2 * RUN_SECONDS_CEILING;
const TOKEN_BYTES: usize = 32;
const SETTING: &str = "prequeue_one_slot";

/// What a job does with a slot held for a hand-in's checks before queueing: holds one itself while its caller lives, joins the one `slot` a caller holds with its `token`, or neither.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Hold {
    #[default]
    None,
    Take { host: String },
    Join { slot: String, token: String },
}

/// The slot on `host` that `value`'s `HOLD_ENV` names for this run to join: only under a landing's carried context and while `prequeue_one_slot` is on, so a run with no such context takes a ticket of its own.
fn joined(host: &str, value: impl Fn(&str) -> Option<String>) -> Hold {
    if !matches!(value(carry::LANDING).as_deref(), Some("1" | "after")) || !enabled() {
        return Hold::None;
    }
    let Some(held) = value(HOLD_ENV) else { return Hold::None };
    match held.split(' ').collect::<Vec<_>>().as_slice() {
        [on, slot, token] if *on == host && !slot.is_empty() && !token.is_empty() => Hold::Join { slot: (*slot).to_owned(), token: (*token).to_owned() },
        _ => Hold::None,
    }
}

impl Hold {
    /// The host a hold takes its slot on, or nothing for a run that holds none.
    pub(super) fn taken_on(&self) -> Option<&str> {
        match self {
            Hold::Take { host } => Some(host.as_str()),
            _ => None,
        }
    }

    /// This hold, or for a land check that holds none the slot its caller's `HOLD_ENV` names on `host` for it to join.
    pub(crate) fn or_joined(self, host: &str, class: super::SlotClass, value: impl Fn(&str) -> Option<String>) -> Hold {
        match self {
            Hold::None if class == super::SlotClass::LandCheck => joined(host, value),
            hold => hold,
        }
    }
}

/// The token `hold` opens its slot with: a new one for a hold, a join's own, none for any other run; it travels only at the head of the job's standard input and on the holder's own standard output.
pub(super) fn token(hold: &Hold) -> Result<Option<String>, String> {
    match hold {
        Hold::Take { .. } => super::proof::random_hex(TOKEN_BYTES).map(Some),
        Hold::Join { token, .. } => Ok(Some(token.clone())),
        Hold::None => Ok(None),
    }
}

/// The fields the host reads from the head of the job's standard input for `hold`.
pub(super) fn header(hold: &Hold, token: Option<&str>) -> Vec<(&'static str, String)> {
    match (hold, token) {
        (Hold::Take { .. }, Some(token)) => vec![("holding", "1".to_owned()), ("token", token.to_owned())],
        (Hold::Join { slot, token }, _) => vec![("join", slot.clone()), ("token", token.clone())],
        _ => Vec::new(),
    }
}

/// Copies the host's standard output to this process's standard error, and once the host says it holds its slot prints `HOST SLOT TOKEN`, the value a run joins it by, on standard output and calls `held`.
pub(super) fn announce(remote: impl BufRead, host: &str, token: &str, mut held: impl FnMut()) {
    for line in remote.split(b'\n') {
        let Ok(line) = line else { break };
        let text = String::from_utf8_lossy(&line);
        match text.strip_prefix(HOLDING).and_then(|rest| rest.split(' ').next()) {
            Some(slot) if !slot.is_empty() => {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{host} {slot} {token}").and_then(|()| out.flush());
                held();
            }
            _ => eprintln!("{text}"),
        }
    }
}

fn enabled() -> bool {
    north_base::platform::harness::document(north_base::platform::harness::DEFAULTS).get(SETTING).and_then(|entry| entry.get("value")).and_then(|value| value.as_bool()).unwrap_or(true)
}
