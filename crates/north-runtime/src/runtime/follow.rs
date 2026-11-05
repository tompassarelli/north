use super::failure::{Failure, failed};
use super::layout::{Layout, TREE};
use super::{deploy, update};
use crate::platform::process;
use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const LANDER_SHADOW_MINUTES: &str = "3";
const LANDER_LOCK: &str = "lander-promote.lock";
const LANDER_LOCK_WAIT: Duration = Duration::from_secs(20 * 60);

pub(super) fn follow(layout: &Layout, repo: &Path) -> Result<(), Failure> {
    if !names_north(&origin(repo)) {
        return Ok(());
    }
    let log = OpenOptions::new().create(true).append(true).open(layout.state.join("runtime-follow.log"))?;
    Command::new(std::env::current_exe()?)
        .args(["runtime", "promote"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}

pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let here = update::update(layout);
    std::thread::scope(|scope| {
        let lander = here.is_ok().then(|| scope.spawn(|| follow_lander(layout)));
        let elsewhere = deploy::deploy(layout, &[]);
        let lander = lander.map_or(Ok(()), |held| held.join().unwrap_or_else(|_| Err(failed(1, "lander promotion panicked"))));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundation::shim::write_executable;

    fn candidate(directory: &Path, script: &str) -> std::path::PathBuf {
        let binary = directory.join("north");
        write_executable(&binary, script);
        binary
    }

    #[test]
    fn the_lander_follows_when_the_candidate_replay_matches() {
        let directory = tempfile::tempdir().unwrap();
        let binary = candidate(directory.path(), "#!/bin/sh\necho \"north land promote: /s/lander now points at runtimes/new; restarted north-land*\"\nexit 0\n");
        assert!(lander_verdict(&binary, directory.path()).is_ok());
    }

    #[test]
    fn the_lander_is_held_with_the_named_reason_when_the_candidate_replay_differs() {
        let directory = tempfile::tempdir().unwrap();
        let binary = candidate(directory.path(), "#!/bin/sh\necho \"north land promote: refused: round lane-a on abc recorded landed but the candidate replayed failed; the lander link is unchanged\"\nexit 1\n");
        let held = lander_verdict(&binary, directory.path()).unwrap_err();
        assert_eq!(held.message, "lander link held: round lane-a on abc recorded landed but the candidate replayed failed");
    }

    #[test]
    fn the_lander_is_held_naming_the_missing_binary_when_the_runtime_has_none() {
        let directory = tempfile::tempdir().unwrap();
        let held = lander_verdict(&directory.path().join("absent"), directory.path()).unwrap_err();
        assert!(held.message.starts_with("lander link held: ") && held.message.contains("absent"), "{}", held.message);
    }
}
