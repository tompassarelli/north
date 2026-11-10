use super::failure::{Failure, failed};
use super::layout::{Layout, TREE};
use super::{deploy, update, walk};
use crate::land_offload;
use crate::platform::process;
use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::ScopedJoinHandle;
use std::time::Duration;
use rustix::fs::FlockOperation;

const LANDER_SHADOW_MINUTES: &str = "3";
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

/// The other hosts take the runtime only after it walked the spine here, so a runtime that breaks the core loop is rolled back on this host and reaches no second one; a promotion that found another running leaves its landing, the shipping and the lander to that one.
pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let walks = land_offload::settles_fleet_here();
    let Followed { here, replayed, walked } = update_following_lander(layout, walks);
    if matches!(here, Ok(update::Flight::Joined)) {
        return Ok(());
    }
    let elsewhere = match walks {
        true => walked.unwrap_or_else(|| walk_and_ship(layout)),
        false => Ok(()),
    };
    let lander = replayed.unwrap_or_else(|| follow_lander(layout));
    here.map(|_| ()).and(elsewhere).and(lander)
}

fn walk_and_ship(layout: &Layout) -> Result<(), Failure> {
    walk::ship_if_walked(walk::walked_here(layout), |_| deploy::deploy_live(layout)).map(|_| ())
}

pub(super) struct Followed {
    pub here: Result<update::Flight, Failure>,
    pub replayed: Option<Result<(), Failure>>,
    pub walked: Option<Result<(), Failure>>,
}

/// Each swap moves the lander link after it on a thread of its own, so the lander follows a runtime the moment it is live while the promotion builds its next pass, and, given `walks`, walks the spine on that runtime on another and ships it once it passed: a promotion that swapped several runtimes in while origin/main kept moving walked only the last, so on 2026-11-10 whiterabbit promoted 8fa62d19, 5e483bd4, 120e03ae and 5f943553 with no canary verdict (north#1096). `replayed` and `walked` are none when nothing was swapped.
pub(super) fn update_following_lander(layout: &Layout, walks: bool) -> Followed {
    std::thread::scope(|scope| {
        let (mut replays, mut walking) = (Vec::new(), Vec::new());
        let here = update::update(layout, &mut || {
            replays.push(scope.spawn(|| follow_lander(layout)));
            if walks {
                walking.push(scope.spawn(|| walk_and_ship(layout)));
            }
        });
        Followed { here, replayed: joined(replays, "lander link held: its replay panicked"), walked: joined(walking, "the spine walk panicked") }
    })
}

fn joined(threads: Vec<ScopedJoinHandle<'_, Result<(), Failure>>>, panicked: &str) -> Option<Result<(), Failure>> {
    (!threads.is_empty()).then(|| threads.into_iter().map(|thread| thread.join().unwrap_or_else(|_| Err(failed(1, panicked)))).fold(Ok(()), Result::and))
}

pub(super) fn follow_lander(layout: &Layout) -> Result<(), Failure> {
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
