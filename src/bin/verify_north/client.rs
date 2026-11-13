use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::exec;
use super::notes::{Note, Notes};
use super::processes::Process;
use super::transcript::Transcript;

const QUERY_WAIT: Duration = Duration::from_secs(30);
const PLACEMENT_WAIT: Duration = Duration::from_secs(90);
const PLACEMENT_HOLD_WAIT: Duration = Duration::from_mins(20);
const PLACEMENT_HOLD_RETRY: Duration = Duration::from_secs(60);
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

    pub fn run(&self, arguments: &[&str], timeout: Duration) -> Result<String, String> {
        self.run_with(arguments, &[], timeout)
    }

    pub fn run_with(&self, arguments: &[&str], variables: &[(&str, &Path)], timeout: Duration) -> Result<String, String> {
        exec::stdout(self.command(arguments, variables), timeout, self.notes.as_ref())
    }

    pub fn output_with(&self, arguments: &[&str], variables: &[(&str, &Path)], timeout: Duration) -> Result<String, String> {
        let output = exec::output(self.command(arguments, variables), timeout, self.notes.as_ref())?;
        Ok(format!("exit {}: {}{}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)))
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

    /// The command for `north ARGUMENTS` in the scratch world. Every process it starts runs the judged north: north reads these three variables for the binary a child runs before the live link and PATH, so the lander's notices and checks and the worker's tools judge this build instead of the live runtime (north#1172).
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
            .env("THREADS_DB", self.state.join("threads.db"))
            .env("NORTH_GRAPH_DB", self.state.join("graph.db"));
        command.envs(variables.iter().copied());
        command
    }

    pub fn placed_claude_account(&self) -> Result<String, String> {
        let deadline = Instant::now() + PLACEMENT_HOLD_WAIT;
        loop {
            let said = self.output_with(&["config", "agents", "plan", "--roomiest"], &[], PLACEMENT_WAIT)?;
            if let Some(label) = said.strip_prefix("exit 0: ").and_then(|out| out.lines().next()).map(str::trim).filter(|label| !label.is_empty()) {
                return Ok(label.to_owned());
            }
            if !said.starts_with("exit 1: ") || Instant::now() >= deadline {
                return Err(format!("`north config agents plan --roomiest` placed no worker: {}", said.trim()));
            }
            eprintln!("verify-north: no account to place the worker on yet: {}", said.trim());
            thread::sleep(PLACEMENT_HOLD_RETRY);
        }
    }

    pub fn host_pid(&self, name: &str) -> Result<Option<u32>, String> {
        let sessions = self.json(&["fleet", "list", "--json"])?;
        let session = sessions.as_array().into_iter().flatten().find(|session| session["name"] == name);
        Ok(session.and_then(|session| session["host_pid"].as_u64()).and_then(|pid| u32::try_from(pid).ok()))
    }

    pub fn transcript(&self, name: &str) -> Result<Transcript, String> {
        self.transcript_within(name, QUERY_WAIT)
    }

    pub fn transcript_within(&self, name: &str, limit: Duration) -> Result<Transcript, String> {
        Ok(Transcript::parse(&self.json_within(&["fleet", "show", name, "--json"], limit)?))
    }

    pub fn create_ledger(&self) -> Result<(), String> {
        self.run(&["threads", "list"], QUERY_WAIT).map(drop).map_err(|problem| format!("the state root's ledger, which `north fleet` opens but never creates: {problem}"))
    }

    pub fn forget(&self, name: &str) -> Result<(), String> {
        self.run(&["fleet", "forget", name], QUERY_WAIT).map(drop)
    }

    fn json(&self, arguments: &[&str]) -> Result<Value, String> {
        self.json_within(arguments, QUERY_WAIT)
    }

    fn json_within(&self, arguments: &[&str], limit: Duration) -> Result<Value, String> {
        let text = self.run(arguments, limit)?;
        serde_json::from_str(&text).map_err(|error| format!("`north {}` printed invalid JSON: {error}", arguments.join(" ")))
    }
}
