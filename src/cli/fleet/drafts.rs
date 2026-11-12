//! `north fleet drafts`: each draft computed for an open box before its launch, with its age, check
//! result, tokens, cost and whether main has moved under its files since its base (north#1240).
use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::now_ms;
use crate::dispatch::{drafts, launch_directory};

pub(super) fn run(flags: &[&str]) -> NorthResult<()> {
    if !flags.is_empty() {
        return Err(NorthError::Usage("north fleet drafts".into()));
    }
    let listed = drafts::read(&launch_directory());
    if listed.is_empty() {
        println!("no drafts");
    }
    let now = now_ms() as f64 / 1000.0;
    for draft in &listed {
        println!("{}", drafts::line(draft, &drafts::ground(draft), now));
    }
    Ok(())
}
