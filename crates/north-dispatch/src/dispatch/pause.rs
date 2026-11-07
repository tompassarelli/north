//! `north fleet dispatch pause --reason TEXT` and `north fleet dispatch resume`: the proxy's intended dispatch state on this host,
//! recorded where `north health` compares it with the timer, so a deploy that restarts the timer under a pause is seen.
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use north_base::dispatch_pause::{RECORD, TIMER};

fn record() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north").join(RECORD)
}

/// Why dispatch is paused on this host, while the proxy's pause stands.
pub fn standing() -> Option<String> {
    fs::read_to_string(record()).ok().map(|reason| reason.trim().to_owned())
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

pub fn pause(arguments: &[String]) -> u8 {
    let [flag, reason] = arguments else {
        eprintln!("usage: north fleet dispatch pause --reason TEXT");
        return 2;
    };
    if flag != "--reason" || reason.trim().is_empty() {
        eprintln!("usage: north fleet dispatch pause --reason TEXT");
        return 2;
    }
    let path = record();
    if let Err(error) = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&path, reason.trim())) {
        eprintln!("north fleet dispatch: {}: {error}", path.display());
        return 1;
    }
    timer("stop")
}

pub fn resume() -> u8 {
    let path = record();
    match fs::remove_file(&path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            eprintln!("north fleet dispatch: {}: {error}", path.display());
            return 1;
        }
        _ => {}
    }
    timer("start")
}
