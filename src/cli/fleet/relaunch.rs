//! `north fleet relaunch`: restart a session under another model or effort, on the same conversation within the model's family and on a fresh one that carries the last report when the family changes.
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
