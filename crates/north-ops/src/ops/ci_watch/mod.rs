//! Watchers over red CI on main: `main-red` confirms a failed main run and asks the lander to
//! revert its culprit, `ci-diagnose` comments a two-stage model diagnosis on the open "main is
//! red" issue, and `red-main-notify` tells the culprit lane's session and the proxy.
pub mod diagnose;
pub mod notify;
pub mod sheriff;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::platform::process;

pub const TITLE: &str = "main is red";

pub(crate) struct Call<'a> {
    pub program: &'a str,
    pub args: &'a [&'a str],
    pub input: Option<&'a str>,
    pub env: &'a [(&'a str, &'a str)],
    pub cwd: Option<&'a Path>,
    pub limit: Duration,
}

impl<'a> Call<'a> {
    pub fn new(program: &'a str, args: &'a [&'a str]) -> Self {
        Self { program, args, input: None, env: &[], cwd: None, limit: Duration::from_secs(600) }
    }

    pub fn output(&self) -> Result<String, String> {
        let mut command = Command::new(self.program);
        command.args(self.args).envs(self.env.iter().copied());
        if let Some(cwd) = self.cwd {
            command.current_dir(cwd);
        }
        let verb = format!("{} {}", self.program, self.args.first().unwrap_or(&""));
        let output = process::try_run_command_within(command, self.input.map(str::as_bytes), self.limit)
            .map_err(|error| format!("{verb}: {error}"))?
            .ok_or_else(|| format!("{verb} timed out after {} s", self.limit.as_secs()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.status.success() {
            let detail = format!("{}{stdout}", String::from_utf8_lossy(&output.stderr));
            let detail: String = detail.trim().chars().take(500).collect();
            return Err(format!("{verb} exited {}: {detail}", output.status.code().unwrap_or(-1)));
        }
        Ok(stdout)
    }
}

pub(crate) fn gh(args: &[&str]) -> Result<String, String> {
    Call::new("gh", args).output()
}

pub(crate) fn gh_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T, String> {
    serde_json::from_str(&gh(args)?).map_err(|error| format!("gh {}: {error}", args.first().unwrap_or(&"")))
}
