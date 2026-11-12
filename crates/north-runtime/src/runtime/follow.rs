use super::failure::{Failure, failed};
use super::layout::{Layout, TREE};
use super::{deploy, update, walk};
use crate::land_offload;
use crate::platform::process;
use std::fs::{self, File, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::ScopedJoinHandle;
use std::time::{Duration, Instant};
use rustix::fs::FlockOperation;

const LANDER_SHADOW_MINUTES: &str = "3";
const LANDER_LOCK: &str = "lander-promote.lock";
const LANDER_WANTED: &str = "lander-follow-wanted";
const LANDER_REPLAY_LIMIT: Duration = Duration::from_secs(30 * 60);
const LANDER_RETAKE: Duration = Duration::from_secs(5);
const LANDER_RETAKE_POLL: Duration = Duration::from_millis(20);
const SCOPED: &str = "NORTH_RUNTIME_PROMOTE_SCOPED";
const SYSTEMD_RUN: &str = "systemd-run";
const SCOPE: [&str; 5] = ["--user", "--scope", "--slice=agent.slice", "--quiet", "--collect"];

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

/// The lander's unit runs `north land follow` and systemd kills its whole control group when the lander restarts on a new runtime, so the promote runs in a scope of its own, under agent.slice so its runtime build counts against the agent CPU quota: in the default app.slice it compiled outside the 12-CPU desk bound (north#892).
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

/// The other hosts take the runtime only after it walked the spine here, so a runtime that breaks the core loop is rolled back on this host and reaches no second one; a promotion that found another running leaves its landing, the shipping and the lander to that one.
pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let walks = land_offload::settles_fleet_here();
    let Followed { here, replayed, walked } = update_following_lander(layout, walks, true);
    if matches!(here, Ok(update::Flight::Joined)) {
        return Ok(());
    }
    let elsewhere = match walks {
        true => walked.unwrap_or_else(|| walk_and_ship(layout)),
        false => Ok(()),
    };
    // The settling host follows from walk_and_ship once its canary passed, so the link moves after the canary and before the replay that judges it.
    let lander = match (walks, replayed) {
        (true, _) => Ok(()),
        (false, Some(result)) => result,
        (false, None) => follow_lander(layout),
    };
    here.map(|_| ()).and(elsewhere).and(lander)
}

fn walk_and_ship(layout: &Layout) -> Result<(), Failure> {
    let own = super::own_runtime().map(|(sha, _)| sha);
    walk::handed_to_live(layout, own.as_deref()).unwrap_or_else(|| walked_and_shipped(layout))
}

/// `north runtime walk`: the walk a promotion left to this runtime, which hands it no further.
pub(super) fn walk(layout: &Layout) -> Result<(), Failure> {
    match land_offload::settles_fleet_here() {
        true => walked_and_shipped(layout),
        false => Err(failed(1, "this host does not settle the fleet, so it walks no runtime and ships none; run it on the host that does")),
    }
}

fn walked_and_shipped(layout: &Layout) -> Result<(), Failure> {
    let walked = walk::walked_here(layout);
    let follow = walked.is_ok();
    std::thread::scope(|scope| {
        let following = follow.then(|| scope.spawn(|| follow_lander(layout)));
        let shipped = walk::ship_if_walked(walked, |_| deploy::deploy_live(layout)).map(|_| ());
        let followed = following.map(|handle| handle.join().unwrap_or_else(|_| Err(failed(1, "the lander link follow panicked"))));
        match followed {
            Some(result) => shipped.and(result),
            None => shipped,
        }
    })
}

pub(super) struct Followed {
    pub here: Result<update::Flight, Failure>,
    pub replayed: Option<Result<(), Failure>>,
    pub walked: Option<Result<(), Failure>>,
}

/// Each swap moves the lander link after it once the runtime's canary passed (`walks`, on the settling host) or at once elsewhere, where the runtime only arrives after its canary passed on the settling host; `follow` starts the link on a host that does not walk here, since `north runtime settle` walks and follows from its own `walk_and_ship`. The walk runs on a thread of its own, so the lander follows a runtime the moment its canary passes while the promotion builds its next pass, and, given `walks`, ships it once it passed: a promotion that swapped several runtimes in while origin/main kept moving walked only the last, so on 2026-11-10 whiterabbit promoted 8fa62d19, 5e483bd4, 120e03ae and 5f943553 with no canary verdict (north#1096). `replayed` and `walked` are none when nothing was swapped.
pub(super) fn update_following_lander(layout: &Layout, walks: bool, follow: bool) -> Followed {
    std::thread::scope(|scope| {
        let (mut replays, mut walking) = (Vec::new(), Vec::new());
        let here = update::update(layout, &mut || {
            if walks {
                walking.push(scope.spawn(|| walk_and_ship(layout)));
            } else if follow {
                replays.push(scope.spawn(|| follow_lander(layout)));
            }
        });
        Followed { here, replayed: joined(replays, "lander link held: its replay panicked"), walked: joined(walking, "the spine walk panicked") }
    })
}

fn joined(threads: Vec<ScopedJoinHandle<'_, Result<(), Failure>>>, panicked: &str) -> Option<Result<(), Failure>> {
    (!threads.is_empty()).then(|| threads.into_iter().map(|thread| thread.join().unwrap_or_else(|_| Err(failed(1, panicked)))).fold(Ok(()), Result::and))
}

/// A promotion whose runtime passed its canary asks the replay of an older runtime to stop, so the link follows the newest runtime within seconds rather than after a replay of up to `LANDER_REPLAY_LIMIT`: the promotion holding `LANDER_LOCK` kills that replay and runs the newest next.
pub(super) fn follow_lander(layout: &Layout) -> Result<(), Failure> {
    if fs::symlink_metadata(layout.lander()).is_err() {
        return Ok(());
    }
    request_newest(layout);
    let mut retaking = Duration::ZERO;
    loop {
        let Some(serial) = lander_lock(layout, retaking)? else {
            println!("north runtime lander: another promotion holds {LANDER_LOCK}; it stops its replay of an older runtime and replays the newest at once");
            return Ok(());
        };
        let judged = fs::read_link(layout.current()).ok();
        let binary = layout.current().join(TREE).join("bin/north");
        let verdict = lander_verdict(&binary, &update::checkout(), &mut || superseded(layout, judged.as_deref()));
        drop(serial);
        let newest = fs::read_link(layout.current()).ok();
        if newest == judged {
            return verdict;
        }
        println!("north runtime lander: current moved to {} during the replay, so the newest replays next", newest.as_deref().map_or_else(String::new, |link| link.display().to_string()));
        retaking = LANDER_RETAKE;
    }
}

/// A follow that let go of the lock to read `current` again takes it back within `within`: a child another thread is starting holds a copy of every open descriptor until it execs, the lock just released among them, so a first refusal is not yet another promotion.
fn lander_lock(layout: &Layout, within: Duration) -> Result<Option<File>, Failure> {
    let started = Instant::now();
    loop {
        let held = update::named_lock(layout, LANDER_LOCK, FlockOperation::NonBlockingLockExclusive)?;
        if held.is_some() || started.elapsed() >= within {
            return Ok(held);
        }
        std::thread::sleep(LANDER_RETAKE_POLL);
    }
}

fn request_newest(layout: &Layout) {
    let Some(newest) = fs::read_link(layout.current()).ok() else { return };
    if let Err(error) = fs::write(layout.state.join(LANDER_WANTED), newest.display().to_string()) {
        println!("north runtime lander: could not record that {} wants the link: {error}", newest.display());
    }
}

/// Whether a promotion asked for the runtime current names now, and it is not the one under replay.
fn superseded(layout: &Layout, judged: Option<&Path>) -> bool {
    let current = fs::read_link(layout.current()).ok();
    let wanted = fs::read_to_string(layout.state.join(LANDER_WANTED)).ok();
    current.as_deref() != judged && current.is_some_and(|current| wanted.is_some_and(|wanted| wanted == current.display().to_string()))
}

fn lander_verdict(binary: &Path, checkout: &Path, superseded: &mut dyn FnMut() -> bool) -> Result<(), Failure> {
    let mut command = Command::new(binary);
    command.args(["land", "promote", "--shadow-minutes", LANDER_SHADOW_MINUTES, "--repo"]).arg(checkout);
    let ran = match process::try_run_group_until(command, LANDER_REPLAY_LIMIT, superseded).map_err(|error| failed(1, format!("lander link held: {}: {error}", binary.display())))? {
        process::Ended::Finished(ran) => ran,
        process::Ended::Stalled(stalled) => return Err(failed(1, format!("lander link held: north land promote ran past {} min and was killed{stalled}", LANDER_REPLAY_LIMIT.as_secs() / 60))),
        process::Ended::Stopped => {
            println!("north runtime lander: the replay was stopped, a newer runtime passed its canary");
            return Ok(());
        }
    };
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
mod tests;
