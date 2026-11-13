use std::fs::File;
use std::io::{Seek, Write};
use std::process::Command;

use super::{CommitId, Land, quote};
use crate::capacity::carry;

/// The bundle of `reference` cut at the shipped base, or at each commit in `held` when the host named the commits its checkout holds; a base the head's history does not contain travels in it too, as `base_reference(reference)`, so the host reads every commit the shipped context names.
pub(super) fn bundle(land: &Land, reference: &str, held: Option<&[CommitId]>) -> Result<File, String> {
    let git = || {
        let mut command = Command::new(north_base::foundation::offload::git());
        command.current_dir(land.checkout);
        command
    };
    let file = tempfile::tempfile().map_err(|error| format!("bundle file: {error}"))?;
    let made = git().args(["update-ref", reference, land.head.as_str()]).status().is_ok_and(|status| status.success());
    if !made {
        return Err(format!("cannot name {} for the bundle", land.head.as_str()));
    }
    let moved = land.base.as_ref().filter(|base| !git().args(["merge-base", "--is-ancestor", base.as_str(), land.head.as_str()]).status().is_ok_and(|status| status.success()));
    let carried = moved.filter(|base| git().args(["update-ref", &base_reference(reference), base.as_str()]).status().is_ok_and(|status| status.success()));
    let mut command = git();
    command.args(["bundle", "create", "-q", "-", reference]);
    if carried.is_some() {
        command.arg(base_reference(reference));
    }
    let cut = |commit: &CommitId| if *commit == land.head || carried == Some(commit) { format!("^{}^", commit.as_str()) } else { format!("^{}", commit.as_str()) };
    match (held, moved) {
        (Some(held), _) => command.args(held.iter().map(cut)),
        (None, Some(base)) => {
            let fork = git().args(["merge-base", base.as_str(), land.head.as_str()]).output().ok().map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|sha| !sha.is_empty());
            command.args(fork.map(|sha| cut(&CommitId(sha))))
        }
        (None, None) => command.args(land.base.iter().map(cut)),
    };
    let bundled = command.stdout(file.try_clone().map_err(|error| error.to_string())?).status().is_ok_and(|status| status.success());
    let _ = git().args(["update-ref", "-d", reference]).status();
    let _ = git().args(["update-ref", "-d", &base_reference(reference)]).status();
    if !bundled {
        return Err("git bundle failed".to_owned());
    }
    let mut file = file;
    file.rewind().map_err(|error| error.to_string())?;
    Ok(file)
}

pub(super) fn base_reference(reference: &str) -> String {
    format!("{reference}-base")
}

/// The landing context a land check carries to the offload host as `NAME='value'` words, naming the shipped base and head; empty for any other run. A landing marked `after` or `after-yield` carries the same context; the host's line ranks `after` with the lander's own jobs and `after-yield` below a hand-in's checks.
pub(super) fn carried(land: &Land, value: impl Fn(&str) -> Option<String>) -> String {
    if !matches!(value(carry::LANDING).as_deref(), Some("1" | "after" | "after-yield")) {
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
pub(super) fn spilled(command: &str, carried: &str, pins: &str, secret: &str, confined: bool, bundled: u64) -> Vec<u8> {
    format!("command={}\ncarried={}\npins={}\nproof={}\nconfined={}\nbundled={bundled}\n", quote(command), quote(carried), quote(pins), quote(secret), if confined { "1" } else { "" }).into_bytes()
}

/// Writes `header`, then `bundle`, to the job's standard input through a pipe, so the header's secret is in no file a process on either host could read back, and hands the pipe back to be held open while the caller lives, so the host ends the job once its caller is gone.
pub(super) fn feed(header: Vec<u8>, mut bundle: File, mut stdin: std::process::ChildStdin) -> std::thread::JoinHandle<std::process::ChildStdin> {
    std::thread::spawn(move || {
        let _ = stdin.write_all(&header).and_then(|()| std::io::copy(&mut bundle, &mut stdin).map(drop));
        stdin
    })
}
