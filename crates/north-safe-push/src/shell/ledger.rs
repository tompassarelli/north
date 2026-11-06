use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use north_base::platform::ssh;

pub fn git(checkout: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").current_dir(checkout).args(arguments).stdin(Stdio::null()).output().map_err(|error| format!("git {}: {error}", arguments[0]))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!("git {} failed: {}", arguments[0], String::from_utf8_lossy(&output.stderr).trim()))
    }
}

pub fn patch(checkout: &Path, range: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("git").current_dir(checkout).args(["format-patch", "--stdout", "--no-signature", range]).stdin(Stdio::null()).output().map_err(|error| format!("git format-patch: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!("git format-patch failed: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

pub struct Answer {
    pub stdout: String,
    pub success: bool,
    pub code: Option<i32>,
    pub write_error: Option<std::io::Error>,
}

pub fn send(destination: &str, command: &str, input: &[u8], environment: &BTreeMap<String, String>) -> Result<Answer, String> {
    let argv: Vec<OsString> = ssh::argv(&["ssh".into()], environment, &["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", destination, command]);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("ssh {destination} {command}: {error}; NOT pushed"))?;
    let written = child.stdin.take().map(|mut pipe| pipe.write_all(input));
    let output = child.wait_with_output().map_err(|error| format!("ssh {destination} {command}: {error}"))?;
    Ok(Answer { stdout: String::from_utf8_lossy(&output.stdout).into_owned(), success: output.status.success(), code: output.status.code(), write_error: written.and_then(Result::err) })
}
