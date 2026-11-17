use north_foundation::foundation::failure::{Failure, failed};
use super::layout::Layout;
use super::review_hold::Screen;
use super::scope::own_scope;
use super::{deploy, ship, update};
use north_offload::land_offload;
use north_base::platform::{host, process};
use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;

const SCOPED: &str = "NORTH_RUNTIME_PROMOTE_SCOPED";

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

/// A promotion that found another running leaves its shipping and the lander link to that one.
pub(super) fn promote(layout: &Layout) -> Result<(), Failure> {
    let ships = land_offload::settles_fleet_here();
    let Followed { here, shipped } = update_following_lander(layout, ships);
    if matches!(here, Ok(update::Flight::Joined)) {
        return Ok(());
    }
    let elsewhere = match ships {
        true => shipped.unwrap_or_else(|| ship_live(layout)),
        false => Ok(()),
    };
    let lander = match ships {
        true => Ok(()),
        false => follow_lander(layout),
    };
    here.map(|_| ()).and(elsewhere).and(lander)
}

fn ship_live(layout: &Layout) -> Result<(), Failure> {
    let live = ship::live_here(layout);
    let follow = live.is_ok();
    std::thread::scope(|scope| {
        let following = follow.then(|| scope.spawn(|| follow_lander(layout)));
        let shipped = ship::ship_screened(layout, live, &Screen::here(&update::trusted_store()?), |_, kept| deploy::deploy_live(layout, kept)).map(|_| ());
        let followed = following.map(|handle| handle.join().unwrap_or_else(|_| Err(failed(1, "the lander link follow panicked"))));
        match followed {
            Some(result) => shipped.and(result),
            None => shipped,
        }
    })
}

pub(super) struct Followed {
    pub here: Result<update::Flight, Failure>,
    pub shipped: Option<Result<(), Failure>>,
}

/// Each swap ships the new runtime to the other hosts when `ships`, on the settling host; the lander link follows from `ship_live` there and from `promote` elsewhere.
pub(super) fn update_following_lander(layout: &Layout, ships: bool) -> Followed {
    std::thread::scope(|scope| {
        let mut shipping = Vec::new();
        let here = update::update(layout, &mut || {
            if ships {
                shipping.push(scope.spawn(|| ship_live(layout)));
            }
        });
        let shipped = (!shipping.is_empty()).then(|| shipping.into_iter().map(|thread| thread.join().unwrap_or_else(|_| Err(failed(1, "the runtime ship panicked")))).fold(Ok(()), Result::and));
        Followed { here, shipped }
    })
}

/// Moves the lander link to the runtime current names, on the settling host only when the hold ledger lets that runtime go live.
pub(super) fn follow_lander(layout: &Layout) -> Result<(), Failure> {
    if fs::symlink_metadata(layout.lander()).is_err() {
        return Ok(());
    }
    if land_offload::settles_fleet_here() {
        lander_cleared(layout, &Screen::here(&update::trusted_store()?))?;
    }
    let Ok(current) = fs::read_link(layout.current()) else { return Ok(()) };
    update::relink(&layout.lander(), &current)?;
    println!("north runtime: lander link follows {}", current.display());
    Ok(())
}

/// The lander link on the host that settles the fleet follows only a live runtime `screen` lets go live, so a head whose after-landing run has not ended green, or that a reviewer holds, leaves the link where it is, while a trip found after its promotion does not (Screen::live); every other host only runs what the settling host shipped after the same screen.
pub(super) fn lander_cleared(layout: &Layout, screen: &Screen) -> Result<(), Failure> {
    let Some(live) = layout.live_sha() else { return Ok(()) };
    screen.live(&live, &[host::name().as_str()], north_worker_sandbox::hold_ledger::now()).map(|_| ()).map_err(|why| failed(1, format!("north runtime lander: the lander link stays where it is: {why}")))
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

