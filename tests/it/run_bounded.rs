use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

fn live() -> bool {
    north().args(["10s", "--", "true"]).output().is_ok_and(|probe| probe.status.success())
}

fn north() -> Command {
    let mut command = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
    command.args(["capacity", "bounded"]);
    command
}

fn bounded(arguments: &[&str]) -> Output {
    north().args(arguments).output().unwrap()
}

#[test]
fn run_bounded_reaps_the_timed_out_subtree_and_leaves_root_owned_files_root_owned() {
    if !live() {
        eprintln!("skipped: no systemd user manager with the owner runtime");
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let pid_file = scratch.path().join("host-pid");
    let script = "(awk '/^NSpid:/ {print $2}' /proc/self/status >\"$1\"; exec sleep 30) & wait";
    let status = north().args(["1s", "--", "bash", "-c", script, "bash"]).arg(&pid_file).status().unwrap();
    assert!(!status.success());
    let child = fs::read_to_string(&pid_file).unwrap();
    let alive = Path::new("/proc").join(child.trim());
    assert!(north_core::platform::wait::until(Duration::from_secs(2), || !alive.exists()), "the timed subtree outlived the bound");
    let owner = bounded(&["10s", "--", "stat", "-c", "%U", "/nix/store"]);
    assert_eq!(String::from_utf8_lossy(&owner.stdout).trim(), "root");
}

