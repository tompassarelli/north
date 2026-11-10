//! Clears the temporaries git leaves at the top of a main checkout, which the worker domain cannot unlink (north#1068). `git merge-tree` with a merge driver set creates `.merge_file_XXXXXX` in its working directory and removes them itself; in a domain that refuses removal outside the lane, the create succeeds and the unlink does not, so the file stays in main for good. The wrapper that starts the confined command stays outside the domain and removes them once the command ends.
use std::path::PathBuf;

use super::Context;

const PREFIX: &str = ".merge_file_";

fn is_temporary(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// The directories where a command run from `context` makes git leave those files: the working directory when it lies in a main checkout, and the top of the main checkout of the project it runs in or names.
pub(super) fn places(context: &Context) -> Vec<PathBuf> {
    let projects = super::removable::project(&context.home, &context.cwd).into_iter().chain(context.named_projects.iter().cloned());
    let mains: Vec<PathBuf> = projects.map(|project| project.join("main")).collect();
    let mut places: Vec<PathBuf> = mains.iter().filter(|main| context.cwd.starts_with(main)).map(|_| context.cwd.clone()).collect();
    places.extend(mains);
    places.sort();
    places.dedup();
    places
}

/// Removes the regular files named like git's merge temporaries that lie directly in `places`.
pub(super) fn clear(places: &[PathBuf]) {
    for place in places {
        for entry in std::fs::read_dir(place).into_iter().flatten().flatten() {
            let leftover = is_temporary(&entry.file_name().to_string_lossy()) && entry.file_type().is_ok_and(|kind| kind.is_file());
            if leftover {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Runs `north --worker-sandbox COMMAND` as a child that builds the domain, waits for it, then clears git's leftovers from the places `context` names; the child's exit code is the command's.
pub(super) fn around(command: &str, context: &Context, marker: &str) -> std::process::ExitCode {
    let Ok(north) = std::env::current_exe() else {
        eprintln!("north worker-sandbox: cannot find its own executable");
        return std::process::ExitCode::from(126);
    };
    let status = std::process::Command::new(north).arg(super::FLAG).arg(command).env(marker, "1").status();
    clear(&places(context));
    match status {
        Ok(status) => std::process::ExitCode::from(status.code().map_or(128, |code| u8::try_from(code).unwrap_or(1))),
        Err(error) => {
            eprintln!("north worker-sandbox: {error}");
            std::process::ExitCode::from(126)
        }
    }
}
