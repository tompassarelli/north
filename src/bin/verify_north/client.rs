use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::exec;
use super::notes::Notes;
use super::transcript::Transcript;

const QUERY_WAIT: Duration = Duration::from_secs(30);

pub struct Account {
    pub provider: String,
    pub label: String,
}

pub struct North {
    program: PathBuf,
    directory: PathBuf,
    notes: Option<Notes>,
}

impl North {
    pub fn new(program: PathBuf, directory: PathBuf, notes: Option<Notes>) -> Self {
        Self { program, directory, notes }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn run(&self, arguments: &[&str], timeout: Duration) -> Result<String, String> {
        let mut command = Command::new(&self.program);
        command.args(arguments).current_dir(&self.directory).env("NORTH_FLEET_TEST", "1");
        exec::operator_environment(&mut command);
        exec::stdout(command, timeout, self.notes.as_ref())
    }

    pub fn accounts(&self) -> Result<Vec<Account>, String> {
        let listing = self.run(&["fleet", "accounts"], QUERY_WAIT)?;
        Ok(listing
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                Some(Account { provider: fields.next()?.to_owned(), label: fields.next()?.to_owned() })
            })
            .collect())
    }

    pub fn host_pid(&self, name: &str) -> Result<Option<u32>, String> {
        let sessions = self.json(&["fleet", "list", "--json"])?;
        let session = sessions.as_array().into_iter().flatten().find(|session| session["name"] == name);
        Ok(session.and_then(|session| session["host_pid"].as_u64()).and_then(|pid| u32::try_from(pid).ok()))
    }

    pub fn transcript(&self, name: &str) -> Result<Transcript, String> {
        Ok(Transcript::parse(&self.json(&["fleet", "show", name, "--json"])?))
    }

    pub fn runs(&self) -> Result<Vec<Value>, String> {
        Ok(self.json(&["ledger", "runs", "--json"])?.as_array().cloned().unwrap_or_default())
    }

    pub fn tasks(&self) -> Result<Vec<Value>, String> {
        Ok(self.json(&["ledger", "summary", "--json"])?["tasks"].as_array().cloned().unwrap_or_default())
    }

    pub fn forget(&self, name: &str) -> Result<(), String> {
        self.run(&["fleet", "forget", name], QUERY_WAIT).map(drop)
    }

    fn json(&self, arguments: &[&str]) -> Result<Value, String> {
        let text = self.run(arguments, QUERY_WAIT)?;
        serde_json::from_str(&text).map_err(|error| format!("`north {}` printed invalid JSON: {error}", arguments.join(" ")))
    }
}
