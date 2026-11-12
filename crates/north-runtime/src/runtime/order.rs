use super::canary;
use super::deploy;
use super::failure::{Failure, failed};
use super::follow;
use super::layout::{self, Layout};
use super::pin::{self, Event};
use super::probe::Shell;
use super::update;
use crate::land_offload;

mod hold;

const USAGE: &str = "usage: north runtime order --reason REASON [--commit REV]  promote REV (default origin/main's head), a commit already on origin/main, to this host and every deploy host at once: each host builds or fetches that runtime, runs its canary probes against it before it is live, goes live only on a pass and rewrites its stable pin, and one row per host in runtime-pins.jsonl names the commit, who ordered it, the reason and the canary verdict; a host whose canary fails keeps its runtime and the probe that failed is named; every host, this one included, is moved to the commit even back from a newer runtime, and a host that stays on another runtime is named, recorded as not live and fails the order; run it as the coordinator's step on the host that settles the fleet, since it stops north-stable-hold.service for the run and starts it again";
const PIN: &str = "channel/stable";

pub(super) struct Order {
    pub(super) sha: String,
    pub(super) reason: String,
    pub(super) orderer: String,
}

pub(super) trait Host: Sync {
    fn name(&self) -> &str;
    fn pinned(&self) -> Result<bool, Failure>;
    fn acquire(&self, sha: &str) -> Result<(), Failure>;
    fn canary(&self, sha: &str) -> Result<(), String>;
    fn go_live(&self, sha: &str) -> Result<(), Failure>;
    fn live(&self) -> Option<String>;
    fn write_pin(&self, sha: &str) -> Result<(), Failure>;
    fn settled(&self) {}
}

pub(super) fn command(layout: &Layout, flags: &[String]) -> Result<(), Failure> {
    let (commit, reason) = parse(flags)?;
    if !land_offload::settles_fleet_here() {
        return Err(failed(64, format!("this host does not settle the fleet; run it on the host that does\n{USAGE}")));
    }
    let checkout = update::checkout();
    let sha = resolve(&checkout, commit.as_deref())?;
    let order = Order { sha, reason, orderer: orderer() };
    let name = crate::platform::host::name();
    let here = Here { layout, shell: Shell::Here(name) };
    let deployment = land_offload::deployment();
    if let Some(line) = deployment.skip_line() {
        println!("{line}");
    }
    let others: Vec<Elsewhere> = deployment.destinations.iter().map(|destination| Elsewhere { layout, shell: Shell::Ssh(destination.clone()) }).collect();
    let others: Vec<&dyn Host> = others.iter().map(|other| other as &dyn Host).collect();
    run(layout, &order, &here, &others, hold::release, hold::restore)
}

fn parse(flags: &[String]) -> Result<(Option<String>, String), Failure> {
    let (mut commit, mut reason) = (None, None);
    let mut rest = flags.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--commit", Some(value)) => commit = Some(value.clone()),
            ("--reason", Some(value)) => reason = Some(value.clone()),
            _ => return Err(failed(64, USAGE)),
        }
    }
    match reason.filter(|reason| !reason.trim().is_empty()) {
        Some(reason) => Ok((commit, reason)),
        None => Err(failed(64, USAGE)),
    }
}

fn resolve(checkout: &std::path::Path, commit: Option<&str>) -> Result<String, Failure> {
    update::fetch(checkout)?;
    let named = commit.unwrap_or("origin/main");
    let sha = update::git(checkout, &["rev-parse", "--verify", &format!("{named}^{{commit}}")])?;
    match super::settle::ancestor(checkout, &sha, "origin/main") {
        true => Ok(sha),
        false => Err(failed(1, format!("{} is not an ancestor of origin/main", layout::short(&sha)))),
    }
}

fn orderer() -> String {
    ["NORTH_FLEET_SESSION", "USER"].iter().find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty())).unwrap_or_else(|| "unknown".to_owned())
}

/// The order marks itself in runtime-order.lock before `release` stops the hold unit, so a promotion that takes the promotion lock in the moment between that stop and the order's own take leaves it to the order (north#1171); the order then holds the promotion lock from before the build until `restore` has put the hold unit back, which queues on that lock, and keeps its mark until that unit is seen holding it.
pub(super) fn run<Held>(layout: &Layout, order: &Order, settling: &dyn Host, others: &[&dyn Host], release: impl FnOnce() -> Result<Held, Failure>, restore: impl FnOnce(Held) -> Result<bool, Failure>) -> Result<(), Failure> {
    let ordering = update::order_marked(layout)?;
    let released = release()?;
    let promoting = match update::order_waiting(layout) {
        Ok(promoting) => promoting,
        Err(failure) => {
            let _ = restore(released);
            return Err(failure);
        }
    };
    let ordered = promote_everywhere(layout, order, settling, others);
    let restored = restore(released);
    drop(promoting);
    if matches!(restored, Ok(true)) {
        update::order_handed_over(layout);
    }
    drop(ordering);
    ordered.and(restored.map(|_| ()))
}

/// The settling host builds, judges and goes live first, since every other host is shipped what it built and takes it only once its canary passed there; then every other host judges and promotes on its own while the settling host's lander follows, so a failed canary on one remote host leaves only that host on its old runtime and none waits on the lander's replay.
fn promote_everywhere(layout: &Layout, order: &Order, settling: &dyn Host, others: &[&dyn Host]) -> Result<(), Failure> {
    if let Err(failure) = settling.acquire(&order.sha) {
        for host in std::iter::once(settling).chain(others.iter().copied()) {
            record(layout, order, host.name(), &format!("not run: {}", failure.message), false);
        }
        return Err(failure);
    }
    let pinned = match live_on(layout, order, settling) {
        Ok(pinned) => pinned,
        Err(failure) => {
            for host in others {
                record(layout, order, host.name(), &format!("not run: {} did not go live", settling.name()), false);
            }
            return Err(reported(failure));
        }
    };
    let pinned_here = pin_on(order, settling, pinned).map_err(reported);
    let failures: Vec<Failure> = std::thread::scope(|scope| {
        let following = scope.spawn(|| settling.settled());
        let ordering: Vec<_> = others.iter().map(|host| scope.spawn(move || live_on(layout, order, *host).and_then(|pinned| pin_on(order, *host, pinned)).map_err(reported))).collect();
        let failed = ordering.into_iter().filter_map(|ordered| ordered.join().unwrap_or_else(|_| Err(reported(failed(1, "a host's promotion panicked")))).err()).collect();
        let _ = following.join();
        failed
    });
    pinned_here.and(failures.into_iter().next().map_or(Ok(()), Err))
}

fn reported(failure: Failure) -> Failure {
    eprintln!("north runtime order: {}", failure.message);
    failure
}

fn record(layout: &Layout, order: &Order, host: &str, canary: &str, live: bool) {
    pin::note(layout, &Event::Ordered { sha: order.sha.clone(), host: host.to_owned(), orderer: order.orderer.clone(), reason: order.reason.clone(), canary: canary.to_owned(), live, at: pin::now() });
}

/// Takes `host` live on the ordered runtime once its canary passed there, and says whether the host keeps a stable pin to rewrite.
fn live_on(layout: &Layout, order: &Order, host: &dyn Host) -> Result<bool, Failure> {
    let (name, short) = (host.name(), layout::short(&order.sha));
    let prepared = host.pinned().and_then(|pinned| host.acquire(&order.sha).map(|()| pinned));
    let pinned = match prepared {
        Ok(pinned) => pinned,
        Err(failure) => {
            record(layout, order, name, &format!("not run: {}", failure.message), false);
            return Err(failed(1, format!("{name} stays on its old runtime: {}", failure.message)));
        }
    };
    if let Err(problems) = host.canary(&order.sha) {
        record(layout, order, name, &format!("failed: {problems}"), false);
        return Err(failed(1, format!("{name} stays on its old runtime: {short} failed its canary: {problems}")));
    }
    if let Err(failure) = host.go_live(&order.sha) {
        record(layout, order, name, &format!("passed; not live: {}", failure.message), false);
        return Err(failed(1, format!("{name}: {short} passed its canary but did not go live: {}", failure.message)));
    }
    let live = host.live();
    if live.as_deref() != Some(order.sha.as_str()) {
        let runs = live.as_deref().map_or("a runtime it did not name".to_owned(), |live| layout::short(live).to_owned());
        record(layout, order, name, &format!("passed; not live: {name} runs {runs}"), false);
        return Err(failed(1, format!("{name}: {short} passed its canary but is not live: {name} was kept on {runs}")));
    }
    record(layout, order, name, "passed", true);
    println!("north runtime order: {name}: {short} passed its canary and is live");
    Ok(pinned)
}

fn pin_on(order: &Order, host: &dyn Host, pinned: bool) -> Result<(), Failure> {
    match pinned {
        true => host.write_pin(&order.sha).map_err(|failure| failed(1, format!("{}: {} is live but its stable pin was not written: {}", host.name(), layout::short(&order.sha), failure.message))),
        false => Ok(()),
    }
}

struct Here<'a> {
    layout: &'a Layout,
    shell: Shell,
}

struct Elsewhere<'a> {
    layout: &'a Layout,
    shell: Shell,
}

const PIN_STATE: &str = "s=${XDG_STATE_HOME:-$HOME/.local/state}/north\n";

fn pinned(shell: &Shell) -> Result<bool, Failure> {
    let said = shell.run(&format!("{PIN_STATE}if [ -f \"$s/{PIN}\" ]; then echo pinned; fi\n")).map_err(|why| failed(1, format!("could not read the stable pin: {why}")))?;
    Ok(said.trim() == "pinned")
}

fn write_pin(shell: &Shell, sha: &str) -> Result<(), Failure> {
    let script = format!("{PIN_STATE}mkdir -p \"$s/channel\" && printf '%s\\n' {sha} >\"$s/channel/.stable.new\" && mv \"$s/channel/.stable.new\" \"$s/{PIN}\"\n");
    shell.run(&script).map(|_| ()).map_err(|why| failed(1, why))
}

impl Host for Here<'_> {
    fn name(&self) -> &str {
        self.shell.name()
    }

    fn pinned(&self) -> Result<bool, Failure> {
        pinned(&self.shell)
    }

    fn acquire(&self, sha: &str) -> Result<(), Failure> {
        update::build_ordered(self.layout, sha)
    }

    fn canary(&self, sha: &str) -> Result<(), String> {
        canary::preflight(self.layout, self.name(), sha, self.live().as_deref(), &canary::Live { layout: self.layout, host: &self.shell })
    }

    fn go_live(&self, sha: &str) -> Result<(), Failure> {
        update::go_live_ordered(self.layout, sha)
    }

    fn live(&self) -> Option<String> {
        self.layout.live_sha()
    }

    fn write_pin(&self, sha: &str) -> Result<(), Failure> {
        write_pin(&self.shell, sha)
    }

    fn settled(&self) {
        if let Err(failure) = follow::follow_lander(self.layout) {
            eprintln!("north runtime order: {}", failure.message);
        }
    }
}

impl Host for Elsewhere<'_> {
    fn name(&self) -> &str {
        self.shell.name()
    }

    fn pinned(&self) -> Result<bool, Failure> {
        pinned(&self.shell)
    }

    fn acquire(&self, sha: &str) -> Result<(), Failure> {
        deploy::stage_runtime(self.name(), sha, &self.layout.runtime(sha))
    }

    fn canary(&self, sha: &str) -> Result<(), String> {
        canary::preflight(self.layout, self.name(), sha, self.live().as_deref(), &canary::Live { layout: self.layout, host: &self.shell })
    }

    /// A ship leaves a host on a runtime newer than the one shipped, so an order that finds the host still off its commit has the runtime live there move it, as the settling host moves itself.
    fn go_live(&self, sha: &str) -> Result<(), Failure> {
        deploy::promote_staged(self.name(), sha, &self.layout.runtime(sha))?;
        match self.live().as_deref() == Some(sha) {
            true => Ok(()),
            false => deploy::move_back_to_ordered(self.name(), sha),
        }
    }

    fn live(&self) -> Option<String> {
        super::probe::live_runtime(&self.shell)
    }

    fn write_pin(&self, sha: &str) -> Result<(), Failure> {
        write_pin(&self.shell, sha)
    }
}

#[cfg(test)]
mod tests;
