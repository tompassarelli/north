use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::SESSION_ENV;

const USAGE: &str = "usage: north fleet adopt --name NAME [--if-free] (run from the interactive window that should hold NAME)";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let (name, if_free) = match words {
        ["--name", name] => (*name, false),
        ["--name", name, "--if-free"] | ["--if-free", "--name", name] => (*name, true),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    if let Ok(held) = std::env::var(SESSION_ENV) {
        return Err(NorthError::Refused(format!("this is the fleet session {held}, which already holds its own name; run north fleet adopt from an interactive window")));
    }
    let conversation = fleet.caller_conversation().ok_or_else(|| NorthError::Usage(format!("no provider session id in this environment; {USAGE}")))?;
    let cwd = std::env::current_dir()?;
    let adopted = if if_free { fleet.adopt_if_free(name, &conversation, cwd)? } else { Some(fleet.adopt(name, &conversation, cwd)?) };
    if let Some(record) = adopted {
        println!("{} now reaches this window on account {}", record.name, record.account);
    }
    Ok(())
}
