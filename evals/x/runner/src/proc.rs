use std::env;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output};

use crate::error::{Result, RunError};

const BUILT_IN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// The build can run where the checkout is mounted at another path, so `evals/x/run` names evals/x at run time.
pub fn eval_x() -> PathBuf {
    env::var_os("EVAL_X_DIR").map_or_else(|| PathBuf::from(BUILT_IN), PathBuf::from)
}

/// podman is installed only in the dev shell of evals/x, which `host` enters.
pub fn podman() -> Command {
    let mut command = Command::new(eval_x().join("host"));
    command.arg("podman");
    command
}

pub fn output(command: &mut Command) -> Result<Output> {
    command.output().map_err(|error| failed(command, &error))
}

pub fn status(command: &mut Command) -> Result<ExitStatus> {
    command.status().map_err(|error| failed(command, &error))
}

pub fn spawn(command: &mut Command) -> Result<Child> {
    command.spawn().map_err(|error| failed(command, &error))
}

pub fn create(path: &Path) -> Result<File> {
    File::create(path)
        .map_err(|error| RunError::Failed(format!("cannot write {}: {error}", path.display())))
}

fn failed(command: &Command, error: &io::Error) -> RunError {
    RunError::Failed(format!(
        "cannot run {}: {error}",
        command.get_program().to_string_lossy()
    ))
}
