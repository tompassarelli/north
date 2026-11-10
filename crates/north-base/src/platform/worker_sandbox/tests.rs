use std::path::Path;
use std::process::Command;

use landlock::RulesetStatus;

use super::{Context, confine, executable, lane_only, removable};

const COMMAND: &str = "NORTH_1033_LANE_COMMAND";

fn runnable(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::foundation::shim::write_script(path, "echo fresh-lane\n");
}

/// The session's long-lived domain is built before the lane, so it cannot hold a rule for it; the fresh domain of the command that runs there is built after the lane exists and must keep the lane its text names readable and executable.
#[test]
fn a_lane_created_after_the_session_started_can_exec_its_scripts() {
    if let Some(command) = std::env::var_os(COMMAND) {
        let command = command.to_string_lossy().into_owned();
        let context = Context::from_env(&command);
        assert!(matches!(confine(Some(&removable(&context)), &lane_only::readable_in_lane(&context), Some(&executable(&context))), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
        let output = Command::new("bash").arg("-c").arg(&command).output().unwrap();
        assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    let scratch = tempfile::Builder::new().prefix("north-1033-fresh-lane").tempdir().unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let session = home.join("code/alpha/worktrees/session");
    std::fs::create_dir_all(session.join("target")).unwrap();
    std::fs::create_dir_all(home.join("code/alpha/main")).unwrap();
    let fresh = home.join("code/alpha/worktrees/fresh");
    runnable(&fresh.join("bin/tool"));
    std::fs::create_dir_all(fresh.join("target")).unwrap();
    let command = format!("cd {} && ./bin/tool", fresh.display());
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "platform::worker_sandbox::tests::a_lane_created_after_the_session_started_can_exec_its_scripts", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env(COMMAND, &command)
        .current_dir(&session)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
