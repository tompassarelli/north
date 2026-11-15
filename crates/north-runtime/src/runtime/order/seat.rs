use super::super::bounded;
use north_runtime_records::failure::{Failure, failed};
use super::super::layout::{self, Layout};
use super::super::lifecycle::{self, Canary, Canarying};
use super::super::probe::Shell;
use super::super::review_hold::Screen;
use super::super::settle::ancestor;
use super::super::{canary, update};
use north_offload::land_offload;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const SEAT: &str = "a one-shot unit of the systemd user manager in control.slice, the seat north-runtime-follow.service runs this host's canary from";
const SYSTEMD_RUN: &str = "systemd-run";
const ONE_SHOT: [&str; 7] = ["--user", "--pipe", "--wait", "--quiet", "--collect", "--slice=control.slice", "--property=RuntimeMaxSec=3600"];
pub(super) const SYSTEM_BINS: [&str; 2] = ["/run/current-system/sw/bin", "/run/wrappers/bin"];
const LIMIT: Duration = Duration::from_secs(3600);
const KEPT_LINES: usize = 20;
const SAID: &str = "north runtime order-here: ";
pub(in crate::runtime) const CANARY: &str = "canary";
pub(in crate::runtime) const GO_LIVE: &str = "go-live";

/// Refuses the order before it releases the hold unit or builds anything when this shell cannot start the seat its canary and swap run from.
pub(super) fn reachable() -> Result<(), Failure> {
    let started = Command::new(SYSTEMD_RUN).args(ONE_SHOT).arg("true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output();
    match started {
        Ok(ran) if ran.status.success() => Ok(()),
        Ok(ran) => Err(failed(1, format!("refused: this shell cannot start {SEAT}, so this host's canary has no seat to run from; nothing was built or released: {}", String::from_utf8_lossy(&ran.stderr).trim()))),
        Err(error) => Err(failed(1, format!("refused: this shell cannot start {SEAT}, so this host's canary has no seat to run from; nothing was built or released: {SYSTEMD_RUN}: {error}"))),
    }
}

/// The canary's broker lays mount namespaces, which a caller's Landlock domain refuses (north#1578), so the order runs this host's canary and swap in a unit of the user manager rather than in its own process tree; only the live runtime's own north under the runtimes store runs there, with a fixed PATH.
pub(super) fn one_shot(layout: &Layout, verb: &str, sha: &str) -> Result<Command, String> {
    let runtime = live_runtime(layout)?;
    let firn = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/firn/bin");
    let path = std::iter::once(runtime.clone()).chain([runtime.join("tree/bin")]).chain(SYSTEM_BINS.map(PathBuf::from)).chain([firn]);
    let path = std::env::join_paths(path).map_err(|error| error.to_string())?;
    let mut command = Command::new(SYSTEMD_RUN);
    command.args(ONE_SHOT).arg(format!("--setenv=PATH={}", path.to_string_lossy())).arg("--").arg(runtime.join("north")).args(["runtime", "order-here", verb, sha]).stdin(Stdio::null());
    Ok(command)
}

pub(super) fn run(layout: &Layout, verb: &str, sha: &str) -> Result<(), String> {
    let mut command = one_shot(layout, verb, sha)?;
    let (status, said) = bounded::status_keeping_stderr(&mut command, LIMIT, KEPT_LINES).map_err(|error| format!("{SEAT} did not finish its {verb}: {error}"))?;
    if status.success() {
        return Ok(());
    }
    let reason = said.iter().rev().find_map(|line| line.strip_prefix(SAID)).map(str::to_owned);
    Err(reason.unwrap_or_else(|| format!("{SEAT} ended its {verb} with {status}; it said: {}", said.join(" | "))))
}

fn live_runtime(layout: &Layout) -> Result<PathBuf, String> {
    let live = layout.live_sha().ok_or("this host has no live runtime whose north could run the order's seat")?;
    let store = layout.runtimes().canonicalize().map_err(|error| format!("{}: {error}", layout.runtimes().display()))?;
    let runtime = layout.runtime(&live).canonicalize().map_err(|error| format!("the live runtime {}: {error}", layout::short(&live)))?;
    match runtime.starts_with(&store) && runtime.join("north").is_file() {
        true => Ok(runtime),
        false => Err(format!("the live runtime {} has no north under the runtimes store {}", runtime.display(), store.display())),
    }
}

/// `north runtime order-here canary|go-live SHA`: the seat's half of a running order on the host that settles the fleet, for a commit already on origin/main whose runtime is built here and that the order's screen lets through; go-live swaps only a runtime whose canary passed here.
pub(in crate::runtime) fn here(layout: &Layout, verb: &str, sha: &str) -> Result<(), Failure> {
    let short = layout::short(sha);
    if !land_offload::settles_fleet_here() {
        return Err(failed(64, "this host does not settle the fleet, so it runs no order's canary"));
    }
    if !update::order_running(layout)? {
        return Err(failed(1, "refused: order-here runs only as a step of a running `north runtime order`, and none holds its mark here"));
    }
    let store = update::trusted_store()?;
    if !ancestor(&store, sha, "origin/main") {
        return Err(failed(1, format!("{short} is not an ancestor of origin/main")));
    }
    if !layout::has_programs(&layout.runtime(sha)) {
        return Err(failed(1, format!("{short} is not built on this host")));
    }
    let name = north_base::platform::host::name();
    Screen::here(&store).order(sha, &[name.as_str()], north_runtime_records::hold_ledger::now()).map_err(|why| failed(1, format!("refused: {why}")))?;
    match verb {
        CANARY => canary::preflight(layout, &name, sha, layout.live_sha().as_deref(), &canary::Live { layout, host: &Shell::Here(name.clone()) }).map_err(|problems| failed(1, problems)),
        GO_LIVE if passed(layout, &name, sha) => update::go_live_ordered(layout, sha),
        GO_LIVE => Err(failed(1, format!("refused: {short} has no passed canary on {name}, so it does not go live"))),
        _ => Err(failed(64, format!("order-here takes {CANARY} or {GO_LIVE}, not {verb}"))),
    }
}

fn passed(layout: &Layout, host: &str, sha: &str) -> bool {
    matches!(lifecycle::state_of::<Canarying>(layout, &format!("{host}/{sha}")), Ok(Some(Canary::Passed | Canary::Trusted)))
}
