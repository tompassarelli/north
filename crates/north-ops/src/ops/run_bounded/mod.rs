//! `north run-bounded DURATION -- COMMAND...`: runs an agent-owned background command under one
//! aggregate memory boundary. Stage one re-enters north inside a guard scope, bound to the fleet
//! session scope, so the wrapper's death stops the service; stage two runs the command as a
//! transient service in the shared slice under a runtime limit. The service has no user
//! namespace, so root-owned files stay root.
use std::collections::BTreeSet;
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};


const USAGE: &str = "usage: north run-bounded <duration> -- <command> [args...]\nduration must be 1s..24h (suffix: s, m, h, or d)";
const OWNER: &str = "/run/current-system/sw/bin";
const SLICE: &str = "agent-bounded.slice";
const MEMORY_HIGH: &str = "MemoryHigh=44G";
const MEMORY_MAX: &str = "MemoryMax=48G";
const STAGE: &str = "RUN_BOUNDED_STAGE";
const UNIT: &str = "RUN_BOUNDED_UNIT";
const SERVE: &str = "serve";
const LONGEST: u64 = 86_400;
const MOST_DIGITS: usize = 5;
const SYSTEMD_OWN: [&str; 8] = ["NOTIFY_SOCKET", "INVOCATION_ID", "JOURNAL_STREAM", "MANAGERPID", "SYSTEMD_EXEC_PID", "LISTEN_PID", "LISTEN_FDS", "LISTEN_FDNAMES"];
const UNAVAILABLE: u8 = 127;

#[derive(Debug, Eq, PartialEq)]
struct Request {
    duration: String,
    seconds: u64,
    command: Vec<String>,
}

fn seconds(duration: &str) -> Option<u64> {
    let digits = duration.trim_end_matches(['s', 'm', 'h', 'd']);
    let unit = &duration[digits.len()..];
    if digits.is_empty() || digits.len() > MOST_DIGITS || !digits.bytes().all(|byte| byte.is_ascii_digit()) || unit.len() > 1 {
        return None;
    }
    let scale = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => 86_400,
    };
    let total = digits.parse::<u64>().ok()? * scale;
    (1..=LONGEST).contains(&total).then_some(total)
}

fn parse(arguments: &[String]) -> Option<Request> {
    let [duration, separator, command @ ..] = arguments else { return None };
    (separator == "--" && !command.is_empty()).then_some(())?;
    Some(Request { seconds: seconds(duration)?, duration: duration.clone(), command: command.to_vec() })
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn guard_arguments(request: &Request, unit: &str, scope: Option<&str>, north: &str) -> Vec<String> {
    let mut words = strings(&["--user", "--scope", "--quiet", "--collect", "--same-dir"]);
    if let Some(scope) = scope {
        words.push(format!("--property=BindsTo={scope}"));
        words.push(format!("--property=After={scope}"));
    }
    words.push(format!("--unit={unit}-guard"));
    words.push(format!("--slice={SLICE}"));
    words.extend([north.to_owned(), "run-bounded".to_owned(), request.duration.clone(), "--".to_owned()]);
    words.extend(request.command.iter().cloned());
    words
}

fn forwarded(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !SYSTEMD_OWN.contains(&name)
}

fn serve_arguments(request: &Request, unit: &str, names: &BTreeSet<String>) -> Vec<String> {
    let mut words = strings(&["--user", "--pipe", "--quiet", "--wait", "--collect", "--same-dir", "--expand-environment=no"]);
    for name in names.iter().filter(|name| forwarded(name)) {
        words.extend(["-E".to_owned(), name.clone()]);
    }
    words.extend([
        format!("--unit={unit}"),
        format!("--slice={SLICE}"),
        format!("--property=BindsTo={unit}-guard.scope"),
        format!("--property=After={unit}-guard.scope"),
        format!("--property=RuntimeMaxSec={}s", request.seconds),
    ]);
    words.extend(strings(&["timeout", "--preserve-status", "--signal=TERM", "--kill-after=5s"]));
    words.push(request.duration.clone());
    words.extend(request.command.iter().cloned());
    words
}

fn owner(tool: &str) -> String {
    format!("{OWNER}/{tool}")
}

fn executable(path: &str) -> bool {
    std::fs::metadata(Path::new(path)).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn unit_name() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("agent-bounded-{}{:06}-{}-{}", now.as_secs(), now.subsec_micros(), std::process::id(), now.subsec_nanos() % 32768)
}

fn boundary() -> Result<(), String> {
    let systemctl = owner("systemctl");
    let run = |words: &[&str]| {
        Command::new(&systemctl)
            .args(words)
            .stdout(Stdio::null())
            .status()
            .map_err(|error| format!("run-bounded: {systemctl}: {error}"))
            .and_then(|status| status.success().then_some(()).ok_or_else(|| format!("run-bounded: {systemctl} {} failed: {status}", words.join(" "))))
    };
    run(&["--user", "start", SLICE])?;
    run(&["--user", "set-property", "--runtime", SLICE, MEMORY_HIGH, MEMORY_MAX])
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(request) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if let Some(missing) = ["systemctl", "systemd-run", "timeout"].map(owner).into_iter().find(|tool| !executable(tool)) {
        eprintln!("run-bounded: required owner runtime is unavailable: {missing}");
        return UNAVAILABLE;
    }
    if let Err(message) = boundary() {
        eprintln!("{message}");
        return 1;
    }
    let systemd_run = owner("systemd-run");
    let mut command = Command::new(&systemd_run);
    if env::var(STAGE).as_deref() != Ok(SERVE) {
        let north = env::current_exe().map_or_else(|_| "north".to_owned(), |path| path.display().to_string());
        let unit = unit_name();
        let scope = env::var("NORTH_FLEET_SCOPE").ok().filter(|scope| !scope.is_empty());
        command.args(guard_arguments(&request, &unit, scope.as_deref(), &north)).env(STAGE, SERVE).env(UNIT, &unit);
    } else {
        let Ok(unit) = env::var(UNIT) else {
            eprintln!("run-bounded: {UNIT} is not set for the serve stage");
            return 1;
        };
        let names: BTreeSet<String> = env::vars_os().filter_map(|(name, _)| name.into_string().ok()).filter(|name| name != STAGE && name != UNIT).collect();
        command.args(serve_arguments(&request, &unit, &names)).env_remove(STAGE).env_remove(UNIT);
    }
    let error = command.exec();
    eprintln!("run-bounded: {systemd_run}: {error}");
    UNAVAILABLE
}
