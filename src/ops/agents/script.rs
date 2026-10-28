use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

pub(super) fn exit_code(command: &mut Command, label: &str) -> u8 {
    match command.status() {
        Ok(status) => status.code().map_or(1, |code| code as u8),
        Err(error) => {
            eprintln!("agents: cannot run {label}: {error}");
            127
        }
    }
}

pub(super) fn python_file<S: AsRef<OsStr>>(
    harness: &Path,
    script: &str,
    arguments: impl IntoIterator<Item = S>,
) -> u8 {
    let path = harness.join("harness/lib").join(script);
    exit_code(
        Command::new("python3").arg("-I").arg(&path).args(arguments),
        script,
    )
}

pub(crate) fn run(harness: &Path, script: &str, arguments: &[String]) -> u8 {
    python_file(harness, script, arguments)
}
