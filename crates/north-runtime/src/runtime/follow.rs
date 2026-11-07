use super::failure::{Failure, failed};
use super::layout::{Layout, TREE};
use super::{deploy, update};
use crate::platform::process;
use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const LANDER_SHADOW_MINUTES: &str = "3";
const LANDER_LOCK: &str = "lander-promote.lock";
const LANDER_LOCK_WAIT: Duration = Duration::from_secs(20 * 60);
const SYSTEMD_RUN: &str = "systemd-run";
const SCOPE: [&str; 4] = ["--user", "--scope", "--quiet", "--collect"];

pub(super) fn follow(layout: &Layout, repo: &Path) -> Result<(), Failure> {
    if !names_north(&origin(repo)) {
        return Ok(());
    }
    let log = OpenOptions::new().create(true).append(true).open(layout.state.join("runtime-follow.log"))?;
    own_scope(std::env::current_exe()?)
        .args(["runtime", "promote"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// The lander's unit runs `north land follow` and systemd kills its whole control group when the lander restarts on a new runtime, so the promote runs in a scope of its own.
fn own_scope(north: PathBuf) -> Command {
    let scopes = Command::new(SYSTEMD_RUN).args(SCOPE).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
    if !scopes {
        return Command::new(north);
    }
    let mut command = Command::new(SYSTEMD_RUN);
    command.args(SCOPE).arg("--").arg(north);
    command
}

pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let here = update::update_after_landing(layout);
    std::thread::scope(|scope| {
        let lander = scope.spawn(|| follow_lander(layout));
        let elsewhere = deploy::deploy(layout, &[]);
        let lander = lander.join().unwrap_or_else(|_| Err(failed(1, "lander promotion panicked")));
        here.and(elsewhere).and(lander)
    })
}

fn follow_lander(layout: &Layout) -> Result<(), Failure> {
    if fs::symlink_metadata(layout.lander()).is_err() {
        return Ok(());
    }
    let _serial = update::named_lock_waiting(layout, LANDER_LOCK, LANDER_LOCK_WAIT, "north runtime lander")?;
    let binary = layout.current().join(TREE).join("bin/north");
    lander_verdict(&binary, &update::checkout())
}

fn lander_verdict(binary: &Path, checkout: &Path) -> Result<(), Failure> {
    let ran = Command::new(binary)
        .args(["land", "promote", "--shadow-minutes", LANDER_SHADOW_MINUTES, "--repo"])
        .arg(checkout)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| failed(1, format!("lander link held: {}: {error}", binary.display())))?;
    let stdout = String::from_utf8_lossy(&ran.stdout);
    print!("{stdout}");
    if ran.status.success() {
        println!("north runtime: lander link follows the promoted runtime");
        return Ok(());
    }
    let reason = held_reason(&stdout, &String::from_utf8_lossy(&ran.stderr)).unwrap_or_else(|| format!("north land promote exited {}", ran.status.code().map_or("on a signal".to_owned(), |code| code.to_string())));
    println!("north runtime: lander link held: {reason}");
    Err(failed(1, format!("lander link held: {reason}")))
}

fn held_reason(stdout: &str, stderr: &str) -> Option<String> {
    let refused = stdout.lines().rev().find_map(|line| line.split_once("refused: ").map(|(_, reason)| reason.trim_end_matches("; the lander link is unchanged").to_owned()));
    refused.or_else(|| stdout.lines().chain(stderr.lines()).rev().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned))
}

fn origin(repo: &Path) -> String {
    process::run("git", Some(repo), &["remote", "get-url", "origin"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn names_north(url: &str) -> bool {
    let url = url.trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    url.rsplit(['/', ':']).next() == Some("north")
}
