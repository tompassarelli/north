use std::io;
use std::process::{Child, Command};

use rustix::process::{Pid, Signal, kill_process_group};

pub(super) fn find_tool(name: &str) -> io::Result<std::path::PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("{name} is required on PATH"),
    ))
}

pub(super) fn vncdo(run: &std::path::Path, port: &str) -> Command {
    let mut command = Command::new(run.join("venv/bin/vncdo"));
    command
        .arg("-s")
        .arg(format!("127.0.0.1::{port}"))
        .arg("-t")
        .arg("8");
    command
}

pub(super) fn checked(command: &mut Command) -> io::Result<u8> {
    status_code(command.status()?)
}

pub(super) fn checked_output(command: &mut Command) -> io::Result<std::process::Output> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "command failed: {}",
            command.get_program().to_string_lossy()
        )));
    }
    Ok(output)
}

pub(super) fn status_code(status: std::process::ExitStatus) -> io::Result<u8> {
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1))
}

pub(super) fn terminate_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id())
        && let Some(group) = Pid::from_raw(pid) {
            let _ = kill_process_group(group, Signal::Term);
        }
    let _ = child.wait();
}
