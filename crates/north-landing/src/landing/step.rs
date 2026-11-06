use std::path::Path;
use std::process::{Command, Stdio};

use super::safe_push_state::cancelled;

#[derive(Debug, Eq, PartialEq)]
pub struct Refused(pub u8);

pub type Step<T = ()> = Result<T, Refused>;

fn git(dir: &Path, args: &[&str], stderr: Stdio) -> Step<String> {
    let output = Command::new("git").current_dir(dir).args(args).stderr(stderr).output().map_err(|_| Refused(127))?;
    cancelled()?;
    if !output.status.success() {
        return Err(Refused(exit_code(output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
}

pub fn git_quiet(dir: &Path, args: &[&str]) -> Option<String> {
    git(dir, args, Stdio::null()).ok()
}

pub fn git_read(dir: &Path, args: &[&str]) -> Step<String> {
    git(dir, args, Stdio::inherit())
}

pub fn git_status(dir: &Path, args: &[&str]) -> Step {
    let status = Command::new("git").current_dir(dir).args(args).status().map_err(|_| Refused(127))?;
    cancelled()?;
    if status.success() { Ok(()) } else { Err(Refused(exit_code(status))) }
}

pub fn exit_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(1),
        (None, None) => 1,
    }
}
