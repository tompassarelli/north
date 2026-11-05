use super::World;
use crate::platform::process::run_command_within;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Live {
    pub pace: crate::guard::host::Pace,
}

fn north() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"))
}

fn stdout_of(program: PathBuf, arguments: &[&str], limit: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command.args(arguments);
    let output = run_command_within(command, None, limit)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

impl World for Live {
    fn now(&self) -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn probe(&self) -> Option<String> {
        if let Some(fixture) = self.var("SPAWN_CAPACITY_STATUS") {
            return std::fs::read_to_string(fixture).ok();
        }
        let mut command = Command::new(north());
        command.args(["capacity", "host", "probe", "--class", "agent"]);
        let output = run_command_within(command, None, Duration::from_secs(2))?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn fleet(&self) -> Option<String> {
        let program = self.var("NORTH_BIN").map_or_else(north, PathBuf::from);
        stdout_of(program, &["fleet", "list", "--json"], Duration::from_secs(2))
    }

    fn flow(&self, repo: &str) -> Option<String> {
        stdout_of(north(), &["flow", "row", repo, "pending", "landed_per_hour"], Duration::from_secs(120))
    }

    fn refresh(&self, provider: &str) -> Option<(i32, String)> {
        let mut command = Command::new(north());
        command.args(["agents", "usage", "--gate", provider]);
        let output = run_command_within(command, None, Duration::from_secs(45))?;
        Some((output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).into_owned()))
    }

    fn pace(&self, request: &Value) -> Result<Value, String> {
        (self.pace)(request)
    }

    fn billed(&self, provider: &str) -> Option<String> {
        crate::usage::billed_account(provider).map(|label| format!("{provider}:{label}"))
    }

    fn cpus(&self) -> f64 {
        std::thread::available_parallelism().map_or(1.0, |count| count.get() as f64)
    }

    fn today(&self) -> String {
        let today = crate::foundation::clock::local_today();
        format!("{:04}/{:02}/{:02}", today.year(), u8::from(today.month()), today.day())
    }
}
