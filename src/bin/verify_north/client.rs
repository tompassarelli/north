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
const PLACEMENT_BRIEF: &str = "Surface: shell\nNeeds: laptop\n";

fn placed_account(listing: &str) -> Result<String, String> {
    let target = listing.lines().find_map(|line| line.split_once(": target ").map(|(_, rest)| rest)).ok_or_else(|| format!("`north config agents plan --place` printed no target: {}", listing.trim()))?;
    let mut words = target.split_whitespace();
    let host = words.next().unwrap_or_default();
    let account = words.next().and_then(|word| word.strip_prefix("claude:")).filter(|_| !host.ends_with(':'));
    account.map(str::to_owned).ok_or_else(|| format!("placement launches no Claude session here: {target}"))
}

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

    fn command(&self, arguments: &[&str], variables: &[(&str, &Path)]) -> Command {
        let mut command = Command::new(&self.program);
        exec::operator_environment(&mut command);
        command
            .args(arguments)
            .current_dir(&self.directory)
            .env("NORTH_FLEET_TEST", "1")
            .env("XDG_STATE_HOME", &self.state)
            .env("NORTH_TEST_ROOT", &self.state)
            .env("THREADS_DB", self.state.join("threads.db"));
        command.envs(variables.iter().copied());
        command
    }

    pub fn placed_claude_account(&self) -> Result<String, String> {
        let brief = tempfile::Builder::new().prefix("verify-north-brief").suffix(".md").tempfile().map_err(|error| format!("brief file: {error}"))?;
        std::fs::write(brief.path(), PLACEMENT_BRIEF).map_err(|error| format!("brief file: {error}"))?;
        let path = brief.path().to_string_lossy().into_owned();
        let deadline = Instant::now() + PLACEMENT_HOLD_WAIT;
        loop {
            let listing = self.run(&["config", "agents", "plan", "--place", &path], PLACEMENT_WAIT)?;
            match placed_account(&listing) {
                Err(problem) if problem.contains(" held: ") && Instant::now() < deadline => thread::sleep(PLACEMENT_HOLD_RETRY),
                placed => return placed,
            }
        }
    }

    pub fn host_pid(&self, name: &str) -> Result<Option<u32>, String> {
        let sessions = self.json(&["fleet", "list", "--json"])?;
        let session = sessions.as_array().into_iter().flatten().find(|session| session["name"] == name);
        Ok(session.and_then(|session| session["host_pid"].as_u64()).and_then(|pid| u32::try_from(pid).ok()))
    }

    pub fn transcript(&self, name: &str) -> Result<Transcript, String> {
        Ok(Transcript::parse(&self.json(&["fleet", "show", name, "--json"])?))
    }

    pub fn create_ledger(&self) -> Result<(), String> {
        self.run(&["threads", "list"], QUERY_WAIT).map(drop).map_err(|problem| format!("the state root's ledger, which `north fleet` opens but never creates: {problem}"))
    }

    pub fn forget(&self, name: &str) -> Result<(), String> {
        self.run(&["fleet", "forget", name], QUERY_WAIT).map(drop)
    }

    fn json(&self, arguments: &[&str]) -> Result<Value, String> {
        let text = self.run(arguments, QUERY_WAIT)?;
        serde_json::from_str(&text).map_err(|error| format!("`north {}` printed invalid JSON: {error}", arguments.join(" ")))
    }
}
