use std::fs::File;
use std::io::{Seek, Write};
use std::process::Command;

use super::{CommitId, Land, quote};
use crate::capacity::carry;

pub(super) fn bundle(land: &Land, reference: &str) -> Result<File, String> {
    let git = || {
        let mut command = Command::new("git");
        command.current_dir(land.checkout);
        command
    };
    let file = tempfile::tempfile().map_err(|error| format!("bundle file: {error}"))?;
    let made = git().args(["update-ref", reference, land.head.as_str()]).status().is_ok_and(|status| status.success());
    if !made {
        return Err(format!("cannot name {} for the bundle", land.head.as_str()));
    }
    let mut command = git();
    command.args(["bundle", "create", "-q", "-", reference]);
    match &land.base {
        Some(base) if *base == land.head => command.arg(format!("^{}^", base.as_str())),
        Some(base) => command.arg(format!("^{}", base.as_str())),
        None => &mut command,
    };
    let bundled = command.stdout(file.try_clone().map_err(|error| error.to_string())?).status().is_ok_and(|status| status.success());
    let _ = git().args(["update-ref", "-d", reference]).status();
    if !bundled {
        return Err("git bundle failed".to_owned());
    }
    let mut file = file;
    file.rewind().map_err(|error| error.to_string())?;
    Ok(file)
}

/// The landing context a land check carries to the offload host as `NAME='value'` words, naming the shipped base and head; empty for any other run.
pub(super) fn carried(land: &Land, value: impl Fn(&str) -> Option<String>) -> String {
    if value(carry::LANDING).as_deref() != Some("1") {
        return String::new();
    }
    let words = carry::names(carry::Boundary::Offload).filter_map(|name| {
        let value = match name {
            carry::LAND_BASE => Some(land.base.as_ref().map_or("", CommitId::as_str).to_owned()),
            carry::LAND_HEAD => Some(land.head.as_str().to_owned()),
            _ => value(name),
        };
        value.map(|value| format!("{name}={}", quote(&value)))
    });
    words.collect::<Vec<_>>().join(" ")
}

/// The words of a land check that can outgrow one argument, and the run's secret, written as the shell assignments the host reads from the head of its stdin: the ssh command line and the host's own `sh -c` take a word of 128 KB at most, and the shim that scopes the call carries it twice (north#951).
pub(super) fn spilled(command: &str, carried: &str, pins: &str, secret: &str) -> Vec<u8> {
    format!("command={}\ncarried={}\npins={}\nproof={}\n", quote(command), quote(carried), quote(pins), quote(secret)).into_bytes()
}

/// Writes `header`, then `bundle`, to the job's standard input through a pipe, so the header's secret is in no file a process on either host could read back.
pub(super) fn feed(header: Vec<u8>, mut bundle: File, mut stdin: std::process::ChildStdin) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = stdin.write_all(&header).and_then(|()| std::io::copy(&mut bundle, &mut stdin).map(drop));
    })
}
