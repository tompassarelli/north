use std::path::Path;
use std::process::{Command, Stdio};

pub use north_base::foundation::step::{Refused, Step, cancelled, refuse, request_cancel};
use north_base::platform::process::exit_code;

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
