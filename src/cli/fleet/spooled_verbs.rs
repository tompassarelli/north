use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::{Fleet, Granted};
use crate::fleet::runtime::{SESSION_ENV, SPOOLED_ENV};
use crate::fleet::store::RowKind;

/// A launch or revive the fleet tick runs from a lead's spool prints only to the tick's launch log, so its failure is told to that lead.
pub(super) fn told_on_failure(fleet: &Fleet, words: &[&str], result: NorthResult<()>) -> NorthResult<()> {
    let lead = std::env::var_os(SPOOLED_ENV).and_then(|_| std::env::var(SESSION_ENV).ok()).filter(|lead| !lead.is_empty());
    if let (Err(error), Some(lead)) = (&result, lead) {
        let name = words.iter().position(|word| *word == "--name").and_then(|at| words.get(at + 1)).or_else(|| words.get(1)).copied().unwrap_or_default();
        fleet.tell(&lead, &format!("north fleet {} {name} that you spooled did not start: {}", words.first().copied().unwrap_or_default(), error.user_message()), Delivery::Queued);
    }
    result
}

/// `north fleet launch`: `direct` starts it here, or it is spooled when this process may not write the fleet's session records.
pub(super) fn or_spool(fleet: &Fleet, words: &[&str], direct: fn(&Fleet, &[&str]) -> NorthResult<()>) -> NorthResult<()> {
    if fleet.writes_records() {
        return direct(fleet, words);
    }
    spool(fleet, RowKind::Launch, std::env::var(SESSION_ENV).ok(), words)
}

/// `north fleet revive NAME [--effort LEVEL]` from a process that may not write the fleet's session records.
pub(super) fn revive(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let words: Vec<&str> = std::iter::once(name).chain(words.iter().copied()).collect();
    spool(fleet, RowKind::Revive, std::env::var(SESSION_ENV).ok(), &words)
}

/// A launch or revive from a process that may not write the fleet's session records, such as a lead in the worker sandbox: its words go to the session's spool, and this host's fleet tick runs them as that session when the lead-only grant allows them, which this process checks first so a refusal shows at once.
fn spool(fleet: &Fleet, kind: RowKind, caller: Option<String>, words: &[&str]) -> NorthResult<()> {
    let verb = kind.label().to_ascii_lowercase();
    let caller = caller.filter(|name| !name.is_empty()).ok_or_else(|| NorthError::Usage(format!("north fleet {verb}: this process may not write the fleet's session records and ${SESSION_ENV} is unset, so no session's spool can carry it to this host's fleet tick")))?;
    let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let granted: Result<Granted, String> = match kind {
        RowKind::Launch => fleet.granted_launch(&caller, &words),
        _ => fleet.granted_revive(&caller, &words),
    };
    let granted = granted.map_err(|why| NorthError::Refused(format!("north fleet {verb}: {why}")))?;
    let row = fleet.spool_verb(kind, &caller, &words)?;
    println!("spooled {} row {}: {} {verb}s once this host's fleet tick applies it, under the checks a direct {verb} runs", kind.label(), row.id, granted.name);
    Ok(())
}
