use std::path::Path;

use super::{Arguments, Config, block_on, clippy_first, run_added, say};

pub fn refuse_clippy(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let commands = config.commands(root, base, head)?;
    let Some(plan) = clippy_first::plan(&commands, |command| !config.is_inferred(command)) else { return Ok(()) };
    say("safe-push: running clippy on the affected crates before queueing the lane");
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() };
    let status = block_on(run_added(root.to_owned(), arguments, config, vec![plan.clippy])).map_err(|error| error.to_string())?;
    if status == 0 { Ok(()) } else { Err(format!("clippy on the affected crates exited {status}; fix the diagnostics above")) }
}
