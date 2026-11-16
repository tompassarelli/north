//! `north-switch`: north's off switches, a program of its own so they answer while north's runtime is missing or
//! broken. Installed as `north` by the host configuration, it takes `north fleet off|on` and `north policy off|on` itself, answers `north hook` with nothing while policy is off, and hands every
//! other command to the runtime's `north`.

use std::env;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: north fleet off [--force] | north fleet on | north policy off | north policy on";

fn main() -> ExitCode {
    let invoked = env::args_os().next().map(PathBuf::from).unwrap_or_default();
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(|word| word.to_str().unwrap_or("")).collect();
    match words.as_slice() {
        ["fleet", verb @ ("off" | "on"), rest @ ..] => fleet(verb, rest),
        ["policy", verb @ ("off" | "on"), rest @ ..] => policy(verb, rest),
        ["hook", ..] if north_switch::policy_is_off() => ExitCode::SUCCESS,
        _ if invoked.file_name().is_some_and(|name| name == "north") => runtime(&arguments),
        _ => usage(),
    }
}

fn fleet(verb: &str, rest: &[&str]) -> ExitCode {
    match (verb, rest) {
        ("off", []) => north_switch::off(north_switch::Mode::Graceful),
        ("off", ["--force"]) => north_switch::off(north_switch::Mode::Force),
        ("on", []) => north_switch::on(),
        _ => usage(),
    }
}

fn policy(verb: &str, rest: &[&str]) -> ExitCode {
    if let Some(session) = north_switch::fleet_session() {
        eprintln!("north policy: refused for fleet session {session}; the owner turns north's policy off and on from his own seat");
        return ExitCode::from(77);
    }
    match (verb, rest) {
        ("off", []) => north_switch::policy_off(),
        ("on", []) => north_switch::policy_on(),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(64)
}

fn runtime(arguments: &[OsString]) -> ExitCode {
    let Some(home) = env::var_os("HOME").filter(|home| !home.is_empty()) else {
        eprintln!("north: HOME is unset, so the runtime's north cannot be found; {USAGE}");
        return ExitCode::from(127);
    };
    let north = Path::new(&home).join(".local/state/north/current/north");
    let error = Command::new(&north).args(arguments).env_remove("CODEX_HOME").exec();
    eprintln!("north: the runtime's north at {} does not start ({error}); `north fleet off` and `north fleet on` still work", north.display());
    ExitCode::from(127)
}
