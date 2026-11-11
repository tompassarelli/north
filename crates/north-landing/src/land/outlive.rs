use std::ffi::OsString;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::ExitStatus;

use rustix::process::Pid;

pub const SLICE_ENV: &str = "NORTH_LAND_RUNNER_SLICE";
const WRAPPER: &str = r#"status=$1; shift; "$@"; code=$?; printf '%s\n' "$code" > "$status.tmp" && mv "$status.tmp" "$status"; exit "$code""#;

pub fn wrap(status: &Path, max_secs: Option<u64>, argv: Vec<OsString>) -> Vec<OsString> {
    let _ = fs::remove_file(status);
    let mut wrapped: Vec<OsString> = Vec::new();
    if let Some(slice) = std::env::var_os(SLICE_ENV).filter(|slice| !slice.is_empty()) {
        wrapped.extend(["systemd-run", "--user", "--scope", "--quiet", "--collect", "--slice"].map(OsString::from));
        wrapped.push(slice);
        if let Some(secs) = max_secs {
            wrapped.extend(["--property".into(), format!("RuntimeMaxSec={secs}").into()]);
        }
        wrapped.push("--".into());
    }
    wrapped.extend(["sh", "-c", WRAPPER, "land-adoptable-status"].map(OsString::from));
    wrapped.push(status.into());
    wrapped.extend(argv);
    wrapped
}

pub fn status(path: &Path) -> Option<i32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

pub fn exit_status(path: &Path) -> Option<ExitStatus> {
    status(path).map(|code| ExitStatus::from_raw((code & 0xff) << 8))
}

pub fn alive(group: i32) -> bool {
    Pid::from_raw(group).is_some_and(|group| rustix::process::test_kill_process_group(group).is_ok())
}

pub fn kill(group: i32) {
    if let Some(group) = Pid::from_raw(group) {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Kill);
    }
}
