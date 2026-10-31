use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::platform::process;

pub(super) const LIMIT: Duration = Duration::from_secs(120);
pub(super) const PUSH_LIMIT: Duration = Duration::from_secs(180);
const ENV: [(&str, &str); 3] = [("GIT_TERMINAL_PROMPT", "0"), ("GIT_OPTIONAL_LOCKS", "0"), ("LC_ALL", "C")];

pub(super) struct Ran {
    pub(super) ok: bool,
    pub(super) out: String,
    pub(super) err: String,
}

impl Ran {
    pub(super) fn last_err_line(&self) -> &str {
        self.err.rsplit('\n').next().unwrap_or("")
    }
}

fn spawn(mut command: Command, limit: Duration, extra: &[(&str, &Path)]) -> Ran {
    command.envs(ENV);
    for (name, value) in extra {
        command.env(name, value);
    }
    match process::run_command_within(command, None, limit) {
        Some(output) => Ran {
            ok: output.status.success(),
            out: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            err: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        },
        None => Ran { ok: false, out: String::new(), err: format!("timed out after {}s", limit.as_secs()) },
    }
}

pub(super) fn command(args: &[&str]) -> Ran {
    let mut command = Command::new(args[0]);
    command.args(&args[1..]);
    spawn(command, LIMIT, &[])
}

pub(super) fn git_with(dir: &Path, args: &[&str], limit: Duration, extra: &[(&str, &Path)]) -> Ran {
    let mut command = Command::new("git");
    command.args(["-c", "maintenance.auto=false", "-c", "gc.auto=0", "-C"]).arg(dir).args(args);
    spawn(command, limit, extra)
}

pub(super) fn git(dir: &Path, args: &[&str]) -> Ran {
    git_with(dir, args, LIMIT, &[])
}

pub(super) fn git_slow(dir: &Path, args: &[&str]) -> Ran {
    git_with(dir, args, PUSH_LIMIT, &[])
}

pub(super) fn verified(dir: &Path, reference: &str) -> Option<String> {
    let ran = git(dir, &["rev-parse", "-q", "--verify", reference]);
    ran.ok.then_some(ran.out)
}

pub(super) fn api(endpoint: &str) -> Result<Value, String> {
    let ran = command(&["gh", "api", endpoint, "--paginate", "--slurp"]);
    if !ran.ok {
        return Err(format!("GitHub eligibility unavailable: {}", ran.err));
    }
    serde_json::from_str(&ran.out).map_err(|error| error.to_string())
}

pub(super) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

pub(super) fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

pub(super) fn real(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
