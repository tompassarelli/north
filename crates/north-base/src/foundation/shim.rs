use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Waits out the "Text file busy" window after a test writes a script, when another thread's fork still holds its write fd until that child execs.
pub fn wait_until_executable(script: &Path) {
    while Command::new(script).status().is_err_and(|error| error.kind() == ErrorKind::ExecutableFileBusy) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Writes an executable sh script that does nothing when run with NORTH_SHIM_PROBE set, and waits out the "Text file busy" window by running it so.
pub fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n[ -z \"$NORTH_SHIM_PROBE\" ] || exit 0\n{body}")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    while Command::new(path).env("NORTH_SHIM_PROBE", "1").status().is_err_and(|error| error.kind() == ErrorKind::ExecutableFileBusy) {
        std::thread::sleep(Duration::from_millis(5));
    }
}
