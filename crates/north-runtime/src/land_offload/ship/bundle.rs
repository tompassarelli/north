use std::fs::File;
use std::io::Seek;
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
