use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Waits out the "Text file busy" window after a test writes a script, when another thread's fork still holds its write fd until that child execs.
pub(crate) fn wait_until_executable(script: &Path) {
    while Command::new(script).status().is_err_and(|error| error.kind() == ErrorKind::ExecutableFileBusy) {
        std::thread::sleep(Duration::from_millis(5));
    }
}
