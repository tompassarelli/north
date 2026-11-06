use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::TomTurn;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::ledger::outcome::read::Places;
use crate::ops::flow::typed::typed_messages;

pub(super) fn relay(fleet: &Fleet, id: i64, number: usize, turn: i64) -> NorthResult<String> {
    let relayer = fleet.answerer().ok_or_else(|| NorthError::Rejected("--relayed-from-turn is for a fleet session relaying Tom's answer; Tom answers a gate directly".into()))?;
    let gate = fleet.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    let conversation = fleet.caller_conversation().ok_or_else(|| NorthError::Rejected("cannot tell which conversation this session is, so its convo turns cannot be checked".into()))?;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let places = Places::live(&crate::ledger::threads_database(), &super::scoreboard::state_root());
    let turn = tom_turn(&places.convo, &home, &conversation, gate.opened_ms, turn)?;
    let closure = fleet.relay_gate(id, number, &turn, &relayer)?;
    Ok(format!("gate {id} closed by {}: {}", closure.by.label(), closure.answer))
}

fn tom_turn(index: &Path, home: &Path, conversation: &str, opened_ms: u64, id: i64) -> NorthResult<TomTurn> {
    let since = i64::try_from(opened_ms / 1000).unwrap_or(i64::MAX);
    let typed = typed_messages(index, home, since, i64::try_from(now_ms() / 1000).unwrap_or(i64::MAX).saturating_add(60));
    let found = typed.into_iter().find(|typed| typed.id == id).ok_or_else(|| NorthError::Rejected(format!("convo turn {id} is not a human turn by Tom after the gate opened in the convo store (the store refreshes every few minutes)")))?;
    if Path::new(&found.session).file_stem().and_then(|stem| stem.to_str()) != Some(conversation) {
        return Err(NorthError::Rejected(format!("convo turn {id} is not in this session's own conversation")));
    }
    Ok(TomTurn { id, at_ms: u64::try_from(found.at).unwrap_or(0).saturating_mul(1000), text: found.text })
}
