use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;

const USAGE: &str = "Usage: north fleet spool take | north fleet spool drop ID...";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let spool = fleet.spool();
    match words {
        ["take"] => println!("{}", serde_json::to_string(&spool.pending()?)?),
        ["drop", ids @ ..] if !ids.is_empty() => spool.withdraw_all(&ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>())?,
        _ => return Err(NorthError::Usage(USAGE.into())),
    }
    Ok(())
}
