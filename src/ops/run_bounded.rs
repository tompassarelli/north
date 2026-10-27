use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const SYSTEMCTL: &str = "/run/current-system/sw/bin/systemctl";
const SYSTEMD_RUN: &str = "/run/current-system/sw/bin/systemd-run";
const TIMEOUT: &str = "/run/current-system/sw/bin/timeout";
const SLICE: &str = "agent-bounded.slice";
const MAX_SECONDS: u64 = 86400;
const STAGE: &str = "RUN_BOUNDED_STAGE";
const STAGE_UNIT: &str = "RUN_BOUNDED_UNIT";
const SERVE: &str = "serve";
const SYSTEMD_OWNED: [&str; 8] = [
    "NOTIFY_SOCKET", "INVOCATION_ID", "JOURNAL_STREAM", "MANAGERPID", "SYSTEMD_EXEC_PID", "LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES",
];

struct Usage;

fn usage() -> u8 {
    eprintln!("usage: run-bounded <duration> -- <command> [args...]");
    eprintln!("duration must be 1s..24h (suffix: s, m, h, or d)");
    2
}

fn seconds(duration: &str) -> Option<u64> {
    let digits = duration.trim_end_matches(['s', 'm', 'h', 'd']);
    let unit = &duration[digits.len()..];
    if digits.is_empty() || digits.len() > 5 || unit.len() > 1 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let scale = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => 86400,
    };
    let total = digits.parse::<u64>().ok()? * scale;
    (1..=MAX_SECONDS).contains(&total).then_some(total)
}

fn parse(args: &[String]) -> Result<(&str, u64, &[String]), Usage> {
    let [duration, separator, command @ ..] = args else { return Err(Usage) };
    if separator != "--" || command.is_empty() {
        return Err(Usage);
    }
    Ok((duration, seconds(duration).ok_or(Usage)?, command))
}

fn exec_failure(program: &str, error: &std::io::Error) -> u8 {
    eprintln!("run-bounded: cannot exec {program}: {error}");
    if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
}

fn prepare_slice() -> Result<(), u8> {
    for tool in [SYSTEMCTL, SYSTEMD_RUN, TIMEOUT] {
        if !Path::new(tool).is_file() {
            eprintln!("run-bounded: required owner runtime is unavailable: {tool}");
            return Err(127);
        }
    }
    let start = Command::new(SYSTEMCTL).args(["--user", "start", SLICE]).status();
    let set = Command::new(SYSTEMCTL)
        .args(["--user", "set-property", "--runtime", SLICE, "MemoryHigh=44G", "MemoryMax=48G"])
        .stdout(Stdio::null())
        .status();
    for status in [start, set] {
        let status = status.map_err(|error| exec_failure(SYSTEMCTL, &error))?;
        if !status.success() {
            return Err(status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1));
        }
    }
    Ok(())
}

fn unit_name() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("agent-bounded-{}-{}-{}", now.as_micros(), std::process::id(), now.subsec_nanos() % 32768)
}

fn guard_scope(unit: &str, duration: &str, command: &[String]) -> Command {
    let mut scope = Command::new(SYSTEMD_RUN);
    scope.args(["--user", "--scope", "--quiet", "--collect", "--same-dir"]);
    if let Some(fleet_scope) = std::env::var_os("NORTH_FLEET_SCOPE").filter(|value| !value.is_empty()) {
        for property in ["BindsTo", "After"] {
            let mut argument = OsString::from(format!("--property={property}="));
            argument.push(&fleet_scope);
            scope.arg(argument);
        }
    }
    scope.arg(format!("--unit={unit}-guard")).arg(format!("--slice={SLICE}"));
    scope.arg(std::env::current_exe().unwrap_or_else(|_| "north".into())).args(["run-bounded", duration, "--"]).args(command);
    scope.env(STAGE, SERVE).env(STAGE_UNIT, unit);
    scope
}

fn passed_environment() -> Vec<String> {
    std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| {
            let mut bytes = name.bytes();
            bytes.next().is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
                && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                && ![STAGE, STAGE_UNIT].contains(&name.as_str())
                && !SYSTEMD_OWNED.contains(&name.as_str())
        })
        .collect()
}

fn service(unit: &str, duration: &str, limit: u64, command: &[String]) -> Command {
    let mut service = Command::new(SYSTEMD_RUN);
    service.args(["--user", "--pipe", "--quiet", "--wait", "--collect", "--same-dir"]);
    for name in passed_environment() {
        service.arg("-E").arg(name);
    }
    service.arg(format!("--unit={unit}")).arg(format!("--slice={SLICE}"));
    service.arg(format!("--property=BindsTo={unit}-guard.scope")).arg(format!("--property=After={unit}-guard.scope"));
    service.arg(format!("--property=RuntimeMaxSec={limit}s"));
    service.args([TIMEOUT, "--preserve-status", "--signal=TERM", "--kill-after=5s", duration]).args(command);
    service.env_remove(STAGE).env_remove(STAGE_UNIT);
    service
}

pub(crate) fn run(args: &[String]) -> u8 {
    let Ok((duration, limit, command)) = parse(args) else { return usage() };
    if let Err(code) = prepare_slice() {
        return code;
    }
    let serving = std::env::var(STAGE).is_ok_and(|stage| stage == SERVE);
    let unit = std::env::var(STAGE_UNIT).ok().filter(|_| serving).unwrap_or_else(unit_name);
    let mut next = if serving { service(&unit, duration, limit, command) } else { guard_scope(&unit, duration, command) };
    exec_failure(SYSTEMD_RUN, &next.exec())
}

#[cfg(test)]
mod tests {
    use super::seconds;

    #[test]
    fn durations_span_one_second_to_one_day_with_an_optional_unit() {
        for (text, want) in [("1", 1), ("10s", 10), ("30m", 1800), ("2h", 7200), ("1d", 86400), ("00090", 90), ("24h", 86400)] {
            assert_eq!(seconds(text), Some(want), "{text}");
        }
        for text in ["", "0s", "25h", "2d", "s", "10x", "1ss", "123456", "-5", "1.5h", "10 s", "+5"] {
            assert_eq!(seconds(text), None, "{text}");
        }
    }
}
