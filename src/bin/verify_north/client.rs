use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use super::exec;
use super::notes::{Note, Notes};
use super::processes::Process;

const JUDGED_NORTH: [&str; 3] = ["AGENT_CAPACITY_NORTH", "NORTH_BIN", "NORTH_LAND_NORTH"];

pub struct North {
    program: PathBuf,
    directory: PathBuf,
    state: PathBuf,
    notes: Option<Notes>,
}

impl North {
    pub fn new(program: PathBuf, directory: PathBuf, state: PathBuf, notes: Option<Notes>) -> Self {
        Self { program, directory, state, notes }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn run_in(&self, cwd: &Path, arguments: &[&str], variables: &[(&str, &Path)], path_prefix: Option<&Path>, timeout: Duration) -> Result<String, String> {
        let mut command = self.command(arguments, variables);
        command.current_dir(cwd);
        if let Some(prefix) = path_prefix {
            let current = std::env::var_os("PATH").unwrap_or_default();
            if let Ok(joined) = std::env::join_paths(std::iter::once(prefix.to_path_buf()).chain(std::env::split_paths(&current))) {
                command.env("PATH", joined);
            }
        }
        exec::stdout(command, timeout, self.notes.as_ref())
    }

    pub fn start_with(&self, arguments: &[&str], variables: &[(&str, &Path)], log: &Path) -> Result<Process, String> {
        let output = File::create(log).map_err(|error| format!("{}: {error}", log.display()))?;
        let errors = output.try_clone().map_err(|error| format!("{}: {error}", log.display()))?;
        let mut command = self.command(arguments, variables);
        command.stdin(Stdio::null()).stdout(output).stderr(errors);
        let child = command.spawn().map_err(|error| format!("`north {}`: {error}", arguments.join(" ")))?;
        let process = Process::of(child.id()).ok_or_else(|| format!("`north {}` exited at once; log: {}", arguments.join(" "), log.display()))?;
        if let Some(notes) = &self.notes {
            notes.note(&Note::Child(process));
        }
        thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
        Ok(process)
    }

    fn command(&self, arguments: &[&str], variables: &[(&str, &Path)]) -> Command {
        let mut command = Command::new(&self.program);
        exec::operator_environment(&mut command);
        for pin in JUDGED_NORTH {
            command.env(pin, &self.program);
        }
        command
            .args(arguments)
            .current_dir(&self.directory)
            .env("NORTH_FLEET_TEST", "1")
            .env("XDG_STATE_HOME", &self.state)
            .env("NORTH_TEST_ROOT", &self.state)
            .env("NORTH_GRAPH_DB", self.state.join("graph.db"));
        command.envs(variables.iter().copied());
        command
    }
}
