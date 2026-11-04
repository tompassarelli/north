//! `north fleet interactive`: stop a fleet session and continue its conversation in the provider's own UI.
use std::os::unix::process::CommandExt;
use std::process::Command;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::HostSpec;

pub(super) fn run(fleet: &Fleet, name: &str) -> NorthResult<()> {
    fleet.interactive(name, replace_with)?;
    Ok(())
}

fn replace_with(spec: &HostSpec) -> NorthResult<()> {
    let Some((program, arguments)) = spec.argv.split_first() else { return Err(NorthError::State("the interactive command is empty".into())) };
    let mut command = Command::new(program);
    command.args(arguments).current_dir(&spec.cwd);
    for key in &spec.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    Err(command.exec().into())
}
