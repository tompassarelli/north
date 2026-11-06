use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::SESSION_ENV;

const USAGE: &str = "usage: north fleet adopt --name NAME (run from the interactive window that should hold NAME)";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let ["--name", name] = words else { return Err(NorthError::Usage(USAGE.into())) };
    if let Ok(held) = std::env::var(SESSION_ENV) {
        return Err(NorthError::Refused(format!("this is the fleet session {held}, which already holds its own name; run north fleet adopt from an interactive window")));
    }
    let conversation = fleet.caller_conversation().ok_or_else(|| NorthError::Usage(format!("no provider session id in this environment; {USAGE}")))?;
    let record = fleet.adopt(name, &conversation, std::env::current_dir()?)?;
    println!("{} now reaches this window on account {}", record.name, record.account);
    Ok(())
}
