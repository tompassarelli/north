use std::path::{Path, PathBuf};

use north_base::foundation::step::{Refused, Step};

use crate::shell::Shell;
use crate::shell::files::Lock;

pub struct Claims {
    pub list: String,
    _locks: Vec<Box<dyn Send>>,
}

pub fn claim_output_log() -> Step<Claims> {
    claim(&Shell::live(Path::new(".")))
}

pub fn claim(shell: &Shell) -> Step<Claims> {
    let claimed_before = &shell.settings.output_claimed;
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut locks: Vec<Box<dyn Send>> = Vec::new();
    for target in shell.files.output_logs() {
        let known = format!(" {} {claimed_before} ", join(&claimed));
        if known.contains(&format!(" {} ", target.display())) {
            continue;
        }
        match shell.files.lock_exclusive(&target) {
            Lock::Held(lock) => locks.push(lock),
            Lock::Unavailable => return Err(Refused(1)),
            Lock::Busy => {
                eprintln!(
                    "safe-push: refusing in {}: {} is already the log of another running safe-push; give each landing its own log file. NOT pushed.",
                    shell.settings.working_directory.display(),
                    target.display()
                );
                return Err(Refused(1));
            }
        }
        claimed.push(target);
    }
    let added: String = claimed
        .iter()
        .map(|path| format!(" {}", path.display()))
        .collect();
    Ok(Claims {
        list: format!("{claimed_before}{added}"),
        _locks: locks,
    })
}

fn join(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "output_log_tests.rs"]
mod tests;
