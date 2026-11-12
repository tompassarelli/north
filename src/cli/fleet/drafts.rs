//! `north fleet drafts`: each draft computed for an open box before its launch, with its age, check
//! result, tokens, cost and whether main has moved under its files since its base (north#1240). A draft
//! still running reads its lane's mark on its build host, and its tokens and cost come from the runs ledger.
use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::now_ms;
use crate::dispatch::{drafting, drafts};

pub(super) fn run(flags: &[&str]) -> NorthResult<()> {
    if !flags.is_empty() {
        return Err(NorthError::Usage("north fleet drafts".into()));
    }
    let cfg = crate::pacing::placement::config().map_err(NorthError::State)?;
    let listed = drafting::listed(&cfg, &crate::platform::host::name());
    if listed.is_empty() {
        println!("no drafts");
    }
    let now = now_ms() as f64 / 1000.0;
    for (draft, ground) in &listed {
        println!("{}", drafts::line(draft, ground, now));
    }
    Ok(())
}
