use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::super::tests::{COMMAND, WRITES_CONFINED};

fn confined_in(home: &Path, lane: &Path, command: &str, writes_confined: bool) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", home)
        .env(COMMAND, command)
        .envs(writes_confined.then_some((WRITES_CONFINED, "1")))
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
    let output = confined_in(&home, &lane, command, false);
    assert!(output.status.success(), "a Buck2 daemon could not run an action it built into a buck-out/ its first build made: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_worker_may_reset_its_buck2_daemon_state_and_nothing_else_in_home() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-1116-buckd").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let lane = buck_lane(&home, "lane");
    buck_lane(&home, "other");
    let own = "d=\"$HOME/.buck/buckd/${PWD#/}/v2\" && mkdir -p \"${d:?}\" && echo 1 > \"${d:?}/buckd.pid\" && rm -rf \"${d:?}/prev\" && mkdir \"${d:?}/prev\" && mv \"${d:?}/buckd.pid\" \"${d:?}/prev/\" && rm -rf \"${d:?}\" && mkdir -p \"${d:?}\" && echo 2 > \"${d:?}/buckd.pid\" && test \"$(cat \"${d:?}/buckd.pid\")\" = 2";
    let refused = "! mkdir -p \"$HOME/.buck/buckd/${PWD#/}/../other/v2\" && ! touch \"$HOME/.buck/anything-else\" && ! mkdir \"$HOME/.buck/tmp\" && ! touch \"$HOME/.buckconfig\"";
    let output = confined_in(&home, &lane, &format!("{own} && {refused}"), true);
    assert!(output.status.success(), "a worker could not restart its own lane's Buck2 daemon, or wrote beside its daemon state: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    std::fs::create_dir_all(home.join(".local/state/north/runtimes")).unwrap();
    let runtime = "s=\"$HOME/.local/state/north\" && ! ln -sfn runtimes/older \"$s/current\" && ! touch \"$s/runtime-promote.lock\" && ! mkdir \"$s/runtimes/older\"";
    let output = confined_in(&home, &lane, runtime, true);
    assert!(output.status.success(), "a worker wrote the host's runtime link, its promotion lock or a runtime beside its own state: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
