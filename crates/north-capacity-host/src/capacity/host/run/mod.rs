//! `north capacity host run`: the modules that admit a request, scope its run and record what it left behind.

mod entry;
pub(crate) mod outcome;
pub(crate) mod scope;
mod waiting;

pub(crate) use entry::run_scoped;
pub(crate) use scope::north_command;
