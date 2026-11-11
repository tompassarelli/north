use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::TomTurn;
use crate::fleet::registry::Fleet;
use crate::ledger::outcome::read::Places;
use crate::flow::typed::{Reach, typed_turn, user_turn_stored};

pub(super) fn relay(fleet: &Fleet, id: i64, number: usize, turn: i64) -> NorthResult<String> {
    let relayer = fleet.answerer().ok_or_else(|| NorthError::Refused("--relayed-from-turn is for a fleet session relaying Tom's answer; Tom answers a gate directly".into()))?;
    fleet.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    let conversation = fleet.caller_conversation().ok_or_else(|| NorthError::Refused("cannot tell which conversation this session is, so its convo turns cannot be checked".into()))?;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let places = Places::live(&crate::ledger::threads_database(), &super::scoreboard::state_root());
    let turn = tom_turn(&places.convo, &home, &conversation, turn)?;
    let closure = fleet.relay_gate(id, number, &turn, &relayer)?;
    Ok(format!("gate {id} closed by {}: {}", closure.by.label(), closure.answer))
}

fn tom_turn(index: &Path, home: &Path, conversation: &str, id: i64) -> NorthResult<TomTurn> {
    let Some(found) = typed_turn(index, home, Reach::Human, id) else {
        return Err(NorthError::Refused(if user_turn_stored(index, id) {
            format!("convo turn {id} was not typed by a person: it is a delivered message, a summary or a fleet prompt")
        } else {
            format!("convo turn {id} is not in the convo store (the store refreshes every few minutes)")
        }));
    };
    if Path::new(&found.session).file_stem().and_then(|stem| stem.to_str()) != Some(conversation) {
        return Err(NorthError::Refused(format!("convo turn {id} is not in this session's own conversation")));
    }
    Ok(TomTurn { id, at_ms: u64::try_from(found.at).unwrap_or(0).saturating_mul(1000), text: found.text })
}

#[cfg(test)]
#[path = "gate_relay_tests.rs"]
mod tests;
