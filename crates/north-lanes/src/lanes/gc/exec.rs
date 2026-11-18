use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use north_base::platform::process;

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
        self.err.trim_end().rsplit('\n').next().unwrap_or("")
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

pub(super) fn in_dir(dir: &Path, args: &[&str]) -> Ran {
    let mut command = Command::new(args[0]);
    command.args(&args[1..]).current_dir(dir);
    spawn(command, PUSH_LIMIT, &[])
}

pub(super) fn git_with(dir: &Path, args: &[&str], limit: Duration, extra: &[(&str, &Path)]) -> Ran {
    let mut command = Command::new("git");
    command.args(["--no-optional-locks", "-c", "maintenance.auto=false", "-c", "gc.auto=0", "-C"]).arg(dir).args(args);
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

pub(super) fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

pub(super) fn real(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub(super) const HOUR_MS: f64 = 3_600_000.0;

pub(super) fn newest_mtime_ms(paths: &[std::path::PathBuf]) -> f64 {
    paths
        .iter()
        .filter_map(|path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok())
        .filter_map(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64() * 1000.0)
        .fold(0.0, f64::max)
}

pub(super) fn modified_within(path: &Path, now_ms: i64, hours: f64) -> bool {
    let since = now_ms as f64 - hours * HOUR_MS;
    let listed = git(path, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
    !listed.ok || listed.out.split('\0').filter(|name| !name.is_empty()).any(|name| newest_mtime_ms(&[path.join(name)]) > since)
}
