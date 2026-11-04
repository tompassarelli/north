//! `north fleet relaunch`: restart a session on the same conversation under another model or effort.
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Effort;
use crate::fleet::registry::Fleet;

const USAGE: &str = "Usage: north fleet relaunch NAME [--model M] [--effort low|medium|high|xhigh|max]";

pub(super) fn run(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (mut model, mut effort) = (None, None);
    for pair in words.chunks(2) {
        match pair {
            ["--model", value] => model = Some(*value),
            ["--effort", level] => effort = Some(Effort::parse(level)?),
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    if model.is_none() && effort.is_none() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    println!("{}", fleet.relaunch(name, model, effort)?.name);
    Ok(())
}
