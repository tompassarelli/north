//! `north fleet dispatch pause --reason TEXT --by WHO (--until TIME | --check COMMAND)`, `north fleet dispatch resume`, and the
//! lift check the fleet tick runs each round: the proxy's intended dispatch state on this host, recorded where `north health`
//! compares it with the timer, so a deploy that restarts the timer under a pause is seen. A pause ends when its check exits 0
//! or its expiry comes, and each pause and its end is one row in the pause ledger (north#1136).
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::dispatch_pause::{Pause, TIMER, record_path};
use north_base::foundation::clock::Timestamp;

const USAGE: &str = "usage: north fleet dispatch pause --reason TEXT --by WHO (--until TIME | --check COMMAND)";
const CHECK_SECS: &str = "30";
const LEDGER: &str = "dispatch-pauses.jsonl";

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Why dispatch is paused on this host, while the proxy's pause stands.
pub fn standing() -> Option<String> {
    north_base::dispatch_pause::standing(&home())
}

fn timer(action: &str) -> u8 {
    match Command::new("systemctl").args(["--user", action, TIMER]).status() {
        Ok(status) if status.success() => 0,
        Ok(status) => {
            eprintln!("north fleet dispatch: systemctl --user {action} {TIMER} exited {status}");
            1
        }
        Err(error) => {
            eprintln!("north fleet dispatch: systemctl --user {action} {TIMER}: {error}");
            1
        }
    }
}

pub fn check_passes(check: &str) -> bool {
    Command::new("timeout").args([CHECK_SECS, "sh", "-c", check]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn ledger(home: &Path, event: &str, pause: &Pause, why: &str) {
    let row = serde_json::json!({ "at": Timestamp::now().iso(), "event": event, "reason": pause.reason, "set_by": pause.set_by, "until": pause.until, "check": pause.check, "why": why });
    let path = home.join(".local/state/north").join(LEDGER);
    let appended = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| OpenOptions::new().create(true).append(true).open(&path)).and_then(|mut file| writeln!(file, "{row}"));
    if let Err(error) = appended {
        eprintln!("north fleet dispatch: pause ledger {}: {error}", path.display());
    }
}

fn flags(arguments: &[String]) -> Result<Pause, String> {
    let mut fields = std::collections::BTreeMap::new();
    for pair in arguments.chunks(2) {
        let [flag, value] = pair else { return Err(USAGE.into()) };
        if !["--reason", "--by", "--until", "--check"].contains(&flag.as_str()) || fields.insert(flag.as_str(), value.as_str()).is_some() {
            return Err(USAGE.into());
        }
    }
    let (Some(reason), Some(set_by)) = (fields.get("--reason"), fields.get("--by")) else { return Err(USAGE.into()) };
    Pause::set(reason, set_by, fields.get("--until").copied(), fields.get("--check").copied(), Timestamp::now())
}

pub fn pause(arguments: &[String]) -> u8 {
    let pause = match flags(arguments) {
        Ok(pause) => pause,
        Err(why) => {
            eprintln!("north fleet dispatch: {why}");
            return 2;
        }
    };
    let home = home();
    let path = record_path(&home);
    if let Err(error) = pause.write(&path) {
        eprintln!("north fleet dispatch: {}: {error}", path.display());
        return 1;
    }
    ledger(&home, "pause", &pause, "set");
    timer("stop")
}

pub fn resume() -> u8 {
    let home = home();
    let path = record_path(&home);
    if let Some(pause) = Pause::read(&path) {
        match fs::remove_file(&path) {
            Ok(()) => ledger(&home, "resume", &pause, "resumed by hand"),
            Err(error) => {
                eprintln!("north fleet dispatch: {}: {error}", path.display());
                return 1;
            }
        }
    }
    timer("start")
}

/// The pause recorded under `home` and the path of its record.
pub fn recorded(home: &Path) -> Option<(Pause, PathBuf)> {
    let path = record_path(home);
    Pause::read(&path).map(|pause| (pause, path))
}

/// The pause standing on this host after this round's lift check: a pause whose check passes or whose expiry came ends here,
/// and its timer starts again.
pub fn review() -> Option<String> {
    let home = home();
    let path = record_path(&home);
    let pause = Pause::read(&path)?;
    let Some(why) = pause.lifted_by(Timestamp::now().unix_seconds_f64(), check_passes) else {
        return Some(pause.reason);
    };
    match fs::remove_file(&path) {
        Ok(()) => {
            ledger(&home, "resume", &pause, &why);
            timer("start");
            None
        }
        Err(_) => Some(pause.reason),
    }
}

#[cfg(test)]
#[path = "pause_tests.rs"]
mod tests;
