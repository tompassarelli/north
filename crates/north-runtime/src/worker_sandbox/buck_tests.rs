use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::super::tests::COMMAND;

fn confined_in(home: &Path, lane: &Path, command: &str) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", home)
        .env(COMMAND, command)
        .current_dir(lane)
        .output()
        .unwrap()
}

fn buck_lane(home: &Path, slug: &str) -> PathBuf {
    let lane = home.join("code/alpha/worktrees").join(slug);
    std::fs::create_dir_all(&lane).unwrap();
    std::fs::write(lane.join(".buckconfig"), "[cells]\n  root = .\n").unwrap();
    lane
}

#[test]
fn a_build_output_directory_created_after_the_sandbox_starts_is_executable() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-1116-buck-out").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let lane = buck_lane(&home, "lane");
    let command = "mkdir -p buck-out/v2/gen/root && printf '#!/bin/sh\\nexit 0\\n' > buck-out/v2/gen/root/action && chmod +x buck-out/v2/gen/root/action && ./buck-out/v2/gen/root/action";
    let output = confined_in(&home, &lane, command);
    assert!(output.status.success(), "a Buck2 daemon could not run an action it built into a buck-out/ its first build made: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
