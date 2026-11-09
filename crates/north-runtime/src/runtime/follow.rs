use super::failure::{Failure, failed};
use super::layout::{Layout, TREE};
use super::{deploy, update};
use crate::platform::process;
use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use rustix::fs::FlockOperation;

const LANDER_SHADOW_MINUTES: &str = "3";
const SWAP_POLL: Duration = Duration::from_millis(100);
const LANDER_LOCK: &str = "lander-promote.lock";
const LANDER_REPLAY_LIMIT: Duration = Duration::from_secs(30 * 60);
const SCOPED: &str = "NORTH_RUNTIME_PROMOTE_SCOPED";
const SYSTEMD_RUN: &str = "systemd-run";
const SCOPE: [&str; 4] = ["--user", "--scope", "--quiet", "--collect"];

pub(super) fn follow(layout: &Layout, repo: &Path) -> Result<(), Failure> {
    if !names_north(&origin(repo)) {
        return Ok(());
    }
    let log = OpenOptions::new().create(true).append(true).open(layout.state.join("runtime-follow.log"))?;
    own_scope(std::env::current_exe()?)
        .env(SCOPED, "1")
        .args(["runtime", "promote"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// The lander's unit runs `north land follow` and systemd kills its whole control group when the lander restarts on a new runtime, so the promote runs in a scope of its own.
pub(super) fn own_scope(north: PathBuf) -> Command {
    let scopes = Command::new(SYSTEMD_RUN).args(SCOPE).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
    if !scopes {
        return Command::new(north);
    }
    let mut command = Command::new(SYSTEMD_RUN);
    command.args(SCOPE).arg("--").arg(north);
    command
}

/// A promotion started in a worker's scope froze with it while holding runtime-update.lock, so it runs in a scope of its own.
pub(super) fn promote_detached(layout: &Layout) -> Result<(), Failure> {
    if std::env::var_os(SCOPED).is_some() {
        return promote(layout);
    }
    let status = own_scope(std::env::current_exe()?).env(SCOPED, "1").args(["runtime", "promote"]).status()?;
    match status.success() {
        true => Ok(()),
        false => Err(failed(status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1), "the promotion, run in a scope of its own, failed; its message is above")),
    }
}

pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let before = layout.live_sha();
    let built = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let here = scope.spawn(|| {
            let here = update::update_after_landing(layout);
            built.store(true, Ordering::Release);
            here
        });
        let elsewhere = scope.spawn(|| {
            await_swap(layout, before.as_deref(), &built);
            deploy::deploy_live(layout)
        });
        let here = here.join().unwrap_or_else(|_| Err(failed(1, "runtime update panicked")));
        let lander = follow_lander(layout);
        let elsewhere = elsewhere.join().unwrap_or_else(|_| Err(failed(1, "runtime shipping panicked")));
        here.and(elsewhere).and(lander)
    })
}

/// The other hosts take the runtime as soon as this host swaps to it, not after this host's health check, agents sync and old-runtime collection, which only the next build waits behind.
fn await_swap(layout: &Layout, before: Option<&str>, update_returned: &AtomicBool) {
    while layout.live_sha().as_deref() == before && !update_returned.load(Ordering::Acquire) {
        std::thread::sleep(SWAP_POLL);
    }
}

fn follow_lander(layout: &Layout) -> Result<(), Failure> {
    if fs::symlink_metadata(layout.lander()).is_err() {
        return Ok(());
    }
    let binary = layout.current().join(TREE).join("bin/north");
    loop {
        let Some(serial) = update::named_lock(layout, LANDER_LOCK, FlockOperation::NonBlockingLockExclusive)? else {
            println!("north runtime lander: another promotion holds {LANDER_LOCK} and replays the newest runtime when its replay ends");
            return Ok(());
        };
        let judged = fs::read_link(layout.current()).ok();
        let verdict = lander_verdict(&binary, &update::checkout());
        drop(serial);
        let newest = fs::read_link(layout.current()).ok();
        if newest == judged {
            return verdict;
        }
        println!("north runtime lander: current moved to {} during the replay, so the newest replays next", newest.as_deref().map_or_else(String::new, |link| link.display().to_string()));
    }
}

fn lander_verdict(binary: &Path, checkout: &Path) -> Result<(), Failure> {
    let mut command = Command::new(binary);
    command.args(["land", "promote", "--shadow-minutes", LANDER_SHADOW_MINUTES, "--repo"]).arg(checkout);
    let ran = process::try_run_group_traced_within(command, LANDER_REPLAY_LIMIT)
        .map_err(|error| failed(1, format!("lander link held: {}: {error}", binary.display())))?
        .map_err(|stalled| failed(1, format!("lander link held: north land promote ran past {} min and was killed{stalled}", LANDER_REPLAY_LIMIT.as_secs() / 60)))?;
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
    use super::{Layout, await_swap};
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    const OLD: &str = "0c54814b5c34a736fc44672f7e752782a7dbcd03";
    const NEW: &str = "1613a7786406485376d98593c765ee31cebbf632";

    // KEEP(bug): on 2026-11-07 a landing reached nexus, ccx33 and cpx62 up to 150 s after this host swapped to it, because shipping began only after this host's health check, agents sync and old-runtime collection (north#731)
    #[test]
    fn shipping_starts_when_this_host_swaps_not_when_its_update_returns() {
        let state = tempfile::tempdir().unwrap();
        let layout = Layout { state: state.path().to_owned() };
        symlink(super::super::layout::link(OLD), layout.current()).unwrap();
        let returned = AtomicBool::new(false);
        let started = Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                let staged = layout.state.join("current.tmp");
                symlink(super::super::layout::link(NEW), &staged).unwrap();
                std::fs::rename(&staged, layout.current()).unwrap();
                while !returned.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            await_swap(&layout, Some(OLD), &returned);
            assert!(started.elapsed() < Duration::from_secs(10), "shipping waited for the update to return");
            returned.store(true, Ordering::Release);
        });
    }
}
