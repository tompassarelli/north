use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use toml_edit::DocumentMut;

const RECORD: &str = "dispatch-tick";
const RESIDENT: &str = "dispatch-resident";
const UNIT: &str = "north-dispatch-round";
pub(super) const ROUND_ENV: &str = "NORTH_DISPATCH_ROUND";
const LIMIT: Duration = Duration::from_secs(15 * 60);
pub(super) const EVERY_SECS: f64 = 5.0 * 60.0;
const FREED_AFTER_SECS: f64 = 20.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Pass {
    Started,
    Unstarted(String),
    Running,
    Recent,
    Paused(String),
    Elsewhere(String),
}

/// Starts a dispatch round through `start` unless the last finished round began under five minutes ago with no slot freed since, the host is paused or another host owns dispatch.
pub(super) fn pass(host: &str, cfg: Option<&DocumentMut>, paused: Option<&str>, directory: &Path, now: f64, resident: impl Fn() -> Option<i64>, start: impl FnOnce() -> Pass) -> Pass {
    if let Some(why) = elsewhere(host, cfg) {
        return Pass::Elsewhere(why);
    }
    if let Some(reason) = paused {
        return Pass::Paused(reason.to_owned());
    }
    if last(directory).is_some_and(|last| now - last < EVERY_SECS && !freed(directory, now - last, &resident)) {
        return Pass::Recent;
    }
    start()
}

/// Starts the round begun at `began` unless one runs.
pub(super) fn begin(began: f64) -> Pass {
    if running() {
        return Pass::Running;
    }
    match start(began) {
        Ok(()) => Pass::Started,
        Err(why) => Pass::Unstarted(why),
    }
}

/// Whether the round's unit is active; systemd starts no second unit of that name while one is loaded.
fn running() -> bool {
    Command::new("systemctl").args(["--user", "is-active", "--quiet", UNIT]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

/// Starts `north fleet dispatch` as the transient user unit of the round begun at `began`, outliving the tick that starts it.
fn start(began: f64) -> Result<(), String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = format!("--setenv=PATH={}", std::env::var("PATH").unwrap_or_default());
    let round = format!("--setenv={ROUND_ENV}={began:.0}");
    let unit = format!("--unit={UNIT}");
    let output = Command::new("systemd-run").args(["--user", "--quiet", "--collect", &unit, &path, &round, "--"]).arg(north).args(["fleet", "dispatch"]).stdin(Stdio::null()).output().map_err(|error| format!("systemd-run: {error}"))?;
    match output.status.success() {
        true => Ok(()),
        false => Err(format!("systemd-run {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
    }
}

/// Runs the round begun at `began`, ending it once it outlives its limit with the phase it was in, and records it as finished only when it returns.
pub(super) fn round(began: f64, directory: &Path, resident: impl Fn() -> Option<i64>, dispatch: impl FnOnce() -> u8) -> u8 {
    std::thread::spawn(|| {
        std::thread::sleep(LIMIT);
        eprintln!("north fleet dispatch: the round outlived its limit of {} s in phase {}, so it ends here and the next tick starts another", LIMIT.as_secs(), crate::dispatch::phase::current());
        std::process::exit(3);
    });
    let code = dispatch();
    finished(directory, began, &resident);
    code
}

fn finished(directory: &Path, began: f64, resident: &impl Fn() -> Option<i64>) {
    let recorded = std::fs::create_dir_all(directory).and_then(|()| std::fs::write(directory.join(RECORD), format!("{began:.0}")));
    if let Err(error) = recorded {
        eprintln!("north fleet dispatch: round not recorded in {}: {error}", directory.display());
    }
    let _ = match resident() {
        Some(count) => std::fs::write(directory.join(RESIDENT), count.to_string()),
        None => std::fs::remove_file(directory.join(RESIDENT)),
    };
}

fn freed(directory: &Path, since: f64, resident: &impl Fn() -> Option<i64>) -> bool {
    if since < FREED_AFTER_SECS {
        return false;
    }
    let before: Option<i64> = std::fs::read_to_string(directory.join(RESIDENT)).ok().and_then(|text| text.trim().parse().ok());
    matches!((before, resident()), (Some(before), Some(now)) if now < before)
}

fn elsewhere(host: &str, cfg: Option<&DocumentMut>) -> Option<String> {
    let named = cfg.and_then(|cfg| cfg.get("placement")).and_then(|placement| placement.get("dispatch_host")).and_then(toml_edit::Item::as_str);
    match named {
        Some(named) if named == host => None,
        Some(named) => Some(format!("this host is not the dispatch host, {named}")),
        None => Some("no [placement] dispatch_host names the host that runs dispatch".into()),
    }
}

fn last(directory: &Path) -> Option<f64> {
    std::fs::read_to_string(directory.join(RECORD)).ok()?.trim().parse().ok()
}

#[cfg(test)]
#[path = "dispatch_tick_tests.rs"]
mod tests;
