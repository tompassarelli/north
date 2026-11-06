use std::path::Path;

use super::{Arguments, Config, affected, block_on, clippy_first, run_added, say};

pub fn refuse_clippy(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let mut commands = config.commands(root, base, head)?;
    commands.extend(config.after_set().commands(root, base, head)?);
    let Some(plan) = clippy_first::plan(&commands, |command| !config.is_inferred(command)) else { return Ok(()) };
    say("safe-push: running clippy on the changed crates before queueing the lane");
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() };
    let status = block_on(run_added(root.to_owned(), arguments, config, vec![plan.clippy])).map_err(|error| error.to_string())?;
    if status == 0 {
        return Ok(());
    }
    let crates = affected::scope(root, base, head).map_or_else(|_| "--workspace".to_owned(), |scope| scope.changed_arguments());
    Err(format!("clippy on the changed crates exited {status}; fix the diagnostics above, then rerun: cargo clippy --all-targets {crates} -- -D warnings"))
}
