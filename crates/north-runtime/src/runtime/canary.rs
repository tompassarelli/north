use super::layout::{self, Layout};
use super::lifecycle::{self, Canarying, Canary};
use north_runtime_records::failure::DEFERRED;
use super::pin::{self, Event};
use super::update;
use north_foundation::foundation::exit::NO_VERDICT;
use north_base::platform::ssh_retry::Rerun;
use std::time::{Duration, Instant};

mod baseline;
mod kept;
mod launch;
mod listing;
mod probes;
mod retirement;
mod services;

pub(super) use launch::probe as launch_probe;
pub(super) use listing::print as list_probes;
pub(super) use services::{probe as services_probe, retire as retire_services};
pub(super) use probes::{CoreLoop, Landers, Probe};

/// The scripts of the two probes that ask a broker from a scratch project they make, for the test of the broker that serves them (worker_sandbox/offload_request_tests.rs).
#[cfg(test)]
pub(crate) fn scratch_project_probes(north: &str) -> [String; 2] {
    [probes::depbox_run(north), probes::scratch_landing(north)]
}

const TAIL_LINES: usize = 12;
const TAIL_BYTES: usize = 1200;
const USAGE: u8 = 64;
const HOST_FAULT: u8 = 65;
const NOT_EXECUTABLE: u8 = 126;
const NOT_FOUND: u8 = 127;
const REMOTE_ROLLBACK_LIMIT: Duration = Duration::from_secs(180);

pub(super) enum Ran {
    Exited { code: u8, log: String },
    Unrun(String),
    Unexecuted { code: u8, log: String },
    HostFault(String),
}

pub(super) enum Rollback {
    To(String),
    Deferred(String),
    Failed(String),
}

pub(super) trait Machine: Sync {
    fn probe(&self, probe: &Probe) -> Ran;
    fn roll_back(&self, sha: &str) -> Rollback;
    fn alarm(&self, text: &str);
    fn page(&self, key: &str, text: &str);
    fn rollback_target(&self, sha: &str) -> Option<String>;
    fn launchable_providers(&self) -> Vec<String>;
    fn sessions_reach_hub(&self, host: &str) -> bool;
    fn offloads_to_hub(&self, host: &str) -> bool;
    fn settles_fleet(&self, host: &str) -> bool;
    fn takes_land_checks(&self, host: &str) -> bool;
    fn lander_repositories(&self) -> Option<Landers>;
    fn lander_take_host(&self, _sha: &str) -> Option<String> {
        None
    }
    /// The landing freeze the host holds, as its file reads, or none.
    fn landing_freeze(&self) -> Option<String> {
        None
    }
    /// The probes runtime `sha` judges itself by on `host`, or why it printed none; a machine that asks no runtime answers with the list compiled into the running north.
    fn listed_by(&self, host: &str, sha: &str) -> Result<CoreLoop, String>
    where
        Self: Sized,
    {
        Ok(compiled(host, sha, self))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Skipped,
    Passed,
    Unrun(String),
    RolledBack,
    Kept,
    Stuck,
}

pub(super) fn compiled(host: &str, sha: &str, machine: &impl Machine) -> CoreLoop {
    let landers = machine.lander_repositories();
    let mut plan = probes::core_loop(host, sha, &machine.launchable_providers(), machine.sessions_reach_hub(host), machine.offloads_to_hub(host), machine.settles_fleet(host), landers.as_ref());
    if machine.takes_land_checks(host) {
        plan.probes.push(probes::offloaded_land_check(host, sha));
        plan.probes.push(probes::worker_cargo_run(sha));
    }
    if machine.settles_fleet(host) && let Some(build) = machine.lander_take_host(sha) {
        plan.probes.push(probes::build_host_take(sha, &build));
    }
    plan
}

fn listed(host: &str, sha: &str, short: &str, machine: &impl Machine) -> CoreLoop {
    machine.listed_by(host, sha).unwrap_or_else(|why| {
        println!("north runtime canary: {host}: {short} is judged by the probe list of the runtime judging it: {why}");
        compiled(host, sha, machine)
    })
}

fn run_probes(layout: &Layout, host: &str, sha: &str, plan: CoreLoop, machine: &impl Machine) -> (Vec<Probe>, Vec<Ran>) {
    let started = Instant::now();
    let short = layout::short(sha);
    if !plan.not_applicable.is_empty() {
        println!("north runtime canary: {host}: {short}: not applicable here: {}", plan.not_applicable.join(", "));
    }
    let probes = plan.probes;
    let ran: Vec<Ran> = std::thread::scope(|scope| {
        let handles: Vec<_> = probes.iter().map(|probe| scope.spawn(|| machine.probe(probe))).collect();
        handles.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Ran::Unrun("the probe panicked".to_owned()))).collect()
    });
    pin::note(layout, &Event::Canaried { sha: sha.to_owned(), host: host.to_owned(), at: pin::now(), seconds: i64::try_from(started.elapsed().as_secs()).unwrap_or(i64::MAX) });
    (probes, ran)
}

pub(super) fn check(layout: &Layout, host: &str, sha: &str, machine: &impl Machine) -> Verdict {
    let instance = format!("{host}/{sha}");
    if let Err(failure) = lifecycle::settle_orphans::<Canarying>(layout, &format!("{host}/")) {
        eprintln!("north runtime canary: {host}: {}", failure.message);
    }
    let state = lifecycle::state_of::<Canarying>(layout, &instance).ok().flatten();
    if state.is_some_and(Canary::judged) {
        return Verdict::Skipped;
    }
    let reading = conditions(machine);
    if state == Some(Canary::Unjudged) && unjudged_under(layout, host, sha).as_deref() == Some(reading.as_str()) {
        return Verdict::Skipped;
    }
    let verdict = judge(layout, host, sha, machine);
    if matches!(verdict, Verdict::Unrun(_)) {
        note_unjudged(layout, host, sha, &reading);
    }
    verdict
}

/// What a probe that could not run waits on, read each tick with a file read and a process scan and no write: the landing freeze, the repositories a running lander serves and the providers a worker launches with. A runtime left unjudged is judged again once this differs from the reading its canary ran under, as when the freeze is lifted or a lander starts serving, and not on every tick: on 2026-11-16 a freeze kept the build host take probe unrun and runtime follow reran every probe for the same runtime each 20 s, about 1 GB of scratch writes a run on the laptop (north#1553).
fn conditions(machine: &impl Machine) -> String {
    let landers = machine.lander_repositories().map_or_else(|| "unread".to_owned(), |landers| landers.served.join(" "));
    format!("freeze: {}\nlanders: {landers}\nproviders: {}\n", machine.landing_freeze().unwrap_or_default().trim(), machine.launchable_providers().join(" "))
}

fn unjudged_file(layout: &Layout, host: &str) -> std::path::PathBuf {
    layout.state.join(format!("runtime-canary-unjudged-{}", host.replace('/', "_")))
}

fn unjudged_under(layout: &Layout, host: &str, sha: &str) -> Option<String> {
    let text = std::fs::read_to_string(unjudged_file(layout, host)).ok()?;
    text.strip_prefix(sha)?.strip_prefix('\n').map(str::to_owned)
}

fn note_unjudged(layout: &Layout, host: &str, sha: &str, reading: &str) {
    if let Err(error) = std::fs::write(unjudged_file(layout, host), format!("{sha}\n{reading}")) {
        eprintln!("north runtime canary: {host}: could not record what {} waits on: {error}", layout::short(sha));
    }
}

fn judge(layout: &Layout, host: &str, sha: &str, machine: &impl Machine) -> Verdict {
    let instance = format!("{host}/{sha}");
    let short = layout::short(sha);
    let finish = |state: Canary, cause: &str| {
        if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, state, cause) {
            eprintln!("north runtime canary: {host}: {short}: {}", failure.message);
        }
    };
    if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, Canary::Judging, "running the core-loop probes on the host") {
        println!("north runtime canary: {host}: {short} not judged here: {}", failure.message);
        return Verdict::Skipped;
    }
    let (probes, ran) = run_probes(layout, host, sha, listed(host, sha, short, machine), machine);
    let tallied = tallies(layout, &probes, &ran);
    let Tallies { failures, broken, faults, unrun } = &tallied;
    if failures.is_empty() {
        if !faults.is_empty() {
            return host_fault(host, short, machine, faults, finish);
        }
        if unrun.is_empty() {
            finish(Canary::Passed, &format!("all {} core-loop probes exited 0", probes.len()));
            println!("north runtime canary: {host}: {short} passed {} core-loop probes", probes.len());
            return Verdict::Passed;
        }
        let why = unrun.join("; ");
        println!("north runtime canary: {host}: {short} not judged: {why}");
        finish(Canary::Unjudged, &why);
        return Verdict::Unrun(why);
    }
    let failure = format!("{} on {host} for runtime {short}", failures.join("; "));
    if let Some(baseline) = baseline::of(layout, host, sha, broken, machine).filter(|baseline| !baseline.beats(broken)) {
        return stays_live(host, short, &baseline.sha, &failure, &tallied, machine, finish);
    }
    match machine.roll_back(sha) {
        Rollback::To(previous) => {
            finish(Canary::RolledBack, &failure);
            machine.alarm(&format!("north runtime canary: {failure}; {host} rolled back to {} and quarantines {short} until a newer landing", layout::short(&previous)));
            Verdict::RolledBack
        }
        Rollback::Deferred(why) => {
            let why = format!("{failure}; rollback deferred: {why}");
            println!("north runtime canary: {host}: {short} not judged: {why}");
            finish(Canary::Unjudged, &why);
            Verdict::Unrun(why)
        }
        Rollback::Failed(why) => {
            finish(Canary::Stuck, &format!("{failure}; could not roll back: {why}"));
            machine.alarm(&format!("north runtime canary: {failure}; {host} could not roll back: {why}"));
            Verdict::Stuck
        }
    }
}

/// Judges a runtime that is not live yet: every core-loop probe of its own list must exit 0 against `runtimes/SHA`, and nothing is rolled back, quarantined or alarmed, since the host's live runtime was never replaced. The probes it adds to and drops from the list of `live`, the runtime live on the host, are printed before any runs and kept in the verdict's cause. The error names each probe that failed or could not run.
pub(super) fn preflight(layout: &Layout, host: &str, sha: &str, live: Option<&str>, machine: &impl Machine) -> Result<(), String> {
    let instance = format!("{host}/{sha}");
    let short = layout::short(sha);
    match lifecycle::state_of::<Canarying>(layout, &instance).ok().flatten() {
        Some(Canary::Passed | Canary::Trusted) => return Ok(()),
        Some(state) if state.judged() => return Err(format!("{short} was already judged {state:?} on {host}")),
        _ => {}
    }
    lifecycle::move_to::<Canarying>(layout, &instance, Canary::Judging, "running the core-loop probes before the runtime is live").map_err(|failure| failure.message)?;
    let plan = listed(host, sha, short, machine);
    let change = listing::against_live(host, sha, &plan, live, machine);
    println!("north runtime canary: {host}: {short}: {change}");
    let (probes, ran) = run_probes(layout, host, sha, plan, machine);
    let ran = release_unrun_cargo_run(host, short, &probes, ran);
    let Tallies { failures, faults, unrun, .. } = tallies(layout, &probes, &ran);
    let problems: Vec<String> = failures.into_iter().chain(faults).chain(unrun).collect();
    let (state, cause) = match problems.is_empty() {
        true => (Canary::Passed, format!("all {} core-loop probes exited 0 before the runtime went live; {change}", probes.len())),
        false => (Canary::Unjudged, problems.join("; ")),
    };
    lifecycle::move_to::<Canarying>(layout, &instance, state, &cause).map_err(|failure| failure.message)?;
    match problems.is_empty() {
        true => Ok(()),
        false => Err(problems.join("; ")),
    }
}

/// A worker cargo run that admission deferred or that passed its time bound is reported as not run and does not hold the promotion, so a repair is never kept off the host it repairs; one the judged runtime could not execute (exit 64, 126 or 127) failed.
fn release_unrun_cargo_run(host: &str, short: &str, probes: &[Probe], ran: Vec<Ran>) -> Vec<Ran> {
    probes.iter().zip(ran).map(|(probe, ran)| match ran {
        Ran::Unrun(why) if probe.name == probes::WORKER_CARGO_RUN_PROBE => {
            println!("north runtime canary: {host}: {short}: probe `{}` not run, so it does not hold the promotion: {why}", probe.name);
            Ran::Exited { code: 0, log: String::new() }
        }
        Ran::Unexecuted { code, log } if probe.name == probes::WORKER_CARGO_RUN_PROBE => Ran::Exited { code, log },
        other => other,
    }).collect()
}

struct Tallies {
    failures: Vec<String>,
    broken: Vec<baseline::Broken>,
    faults: Vec<String>,
    unrun: Vec<String>,
}

fn tallies(layout: &Layout, probes: &[Probe], ran: &[Ran]) -> Tallies {
    let mut tallies = Tallies { failures: Vec::new(), broken: Vec::new(), faults: Vec::new(), unrun: Vec::new() };
    for (probe, ran) in probes.iter().zip(ran) {
        match ran {
            Ran::Exited { code: 0, .. } => {}
            Ran::Exited { code, log } => {
                let kept = kept::failure(layout, log).map_or_else(String::new, |kept| format!("; its failing step is kept in {}", kept.display()));
                tallies.failures.push(format!("probe `{}` exited {code}{kept}; log tail: {}", probe.name, tail(log)));
                tallies.broken.push(baseline::Broken::of(probe, log));
            }
            Ran::Unrun(why) => tallies.unrun.push(format!("probe `{}` not run: {why}", probe.name)),
            Ran::Unexecuted { code, log } => tallies.unrun.push(format!("probe `{}` not run: {}", probe.name, unexecuted(*code, log))),
            Ran::HostFault(why) => tallies.faults.push(format!("probe `{}` found a host fault: {why}", probe.name)),
        }
    }
    tallies
}

fn stays_live(host: &str, short: &str, baseline: &str, failure: &str, tallied: &Tallies, machine: &impl Machine, finish: impl Fn(Canary, &str)) -> Verdict {
    let kept = layout::short(baseline);
    finish(Canary::Kept, &format!("{failure}; {kept}, the runtime it would roll back to, fails there too, so it stays live{}", tallied.faults.iter().chain(&tallied.unrun).map(|why| format!("; {why}")).collect::<String>()));
    println!("north runtime canary: {host}: {short} stays live: {kept}, the runtime it would roll back to, fails the same probes no later");
    machine.page(&baseline::shared_key(host, &tallied.broken), &format!("north runtime canary: {failure}; {kept}, the runtime it would roll back to, fails there too, so {host} keeps {short} live"));
    Verdict::Kept
}

fn host_fault(host: &str, short: &str, machine: &impl Machine, faults: &[String], finish: impl Fn(Canary, &str)) -> Verdict {
    let why = faults.join("; ");
    machine.alarm(&format!("north runtime canary: {host}: {why}; the host is at fault and {short} stays live"));
    println!("north runtime canary: {host}: {short} not judged: {why}");
    finish(Canary::Unjudged, &why);
    Verdict::Unrun(why)
}

pub(super) fn tail(log: &str) -> String {
    let lines: Vec<&str> = log.lines().map(str::trim_end).filter(|line| !line.is_empty()).collect();
    let joined = lines[lines.len().saturating_sub(TAIL_LINES)..].join(" | ");
    let start = joined.len().saturating_sub(TAIL_BYTES);
    let start = (start..=joined.len()).find(|at| joined.is_char_boundary(*at)).unwrap_or(joined.len());
    joined[start..].to_owned()
}

pub(super) fn unexecuted(code: u8, log: &str) -> String {
    format!("the judged runtime could not run the probe (exit {code}): {}", tail(log))
}

pub(super) fn classify(code: Option<i32>, log: String) -> Ran {
    match code.and_then(|code| u8::try_from(code).ok()) {
        Some(HOST_FAULT) => Ran::HostFault(tail(&log)),
        Some(code @ (USAGE | NOT_EXECUTABLE | NOT_FOUND)) => Ran::Unexecuted { code, log },
        Some(NO_VERDICT) => Ran::Unrun(format!("the judged runtime could not run the probe (exit {NO_VERDICT}): {}", tail(&log))),
        Some(code) => Ran::Exited { code, log },
        None => Ran::Unrun(format!("the probe was killed by a signal: {}", tail(&log))),
    }
}

pub(super) struct Live<'a> {
    pub(super) layout: &'a Layout,
    pub(super) host: &'a super::probe::Shell,
}

impl Machine for Live<'_> {
    fn probe(&self, probe: &Probe) -> Ran {
        match self.host.exec(&probe.script, probe.limit, Rerun::BeforeSend) {
            Ok(Some(output)) => classify(output.status.code(), format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))),
            Ok(None) => Ran::Unrun(format!("no verdict within {}s", probe.limit.as_secs())),
            Err(error) => Ran::Unrun(format!("could not start: {error}")),
        }
    }

    fn roll_back(&self, sha: &str) -> Rollback {
        if let super::probe::Shell::Here(_) = self.host {
            return match update::roll_back(self.layout, sha) {
                Ok(previous) => Rollback::To(previous),
                Err(failure) if failure.code == DEFERRED => Rollback::Deferred(failure.message),
                Err(failure) => Rollback::Failed(failure.message),
            };
        }
        let script = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/current/north\" runtime rollback --from {sha}\n");
        match self.host.exec(&script, REMOTE_ROLLBACK_LIMIT, Rerun::BeforeSend) {
            Ok(Some(output)) => match output.status.code().and_then(|code| u8::try_from(code).ok()) {
                Some(0) => Rollback::To(String::from_utf8_lossy(&output.stdout).trim().rsplit(' ').next().unwrap_or_default().to_owned()),
                Some(DEFERRED) => Rollback::Deferred(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
                _ => Rollback::Failed(format!("{}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim())),
            },
            Ok(None) => Rollback::Failed(format!("no answer within {}s", REMOTE_ROLLBACK_LIMIT.as_secs())),
            Err(error) => Rollback::Failed(format!("could not start: {error}")),
        }
    }

    fn alarm(&self, text: &str) {
        update::alert_coordinator(text);
    }

    fn page(&self, key: &str, text: &str) {
        update::page_coordinator(key, text);
    }

    fn rollback_target(&self, sha: &str) -> Option<String> {
        match self.host {
            super::probe::Shell::Here(_) => lifecycle::previous(self.layout, sha, |previous| layout::has_programs(&self.layout.runtime(previous))).ok().flatten(),
            super::probe::Shell::Ssh(_) => None,
        }
    }

    fn launchable_providers(&self) -> Vec<String> {
        let name = self.host.name().rsplit_once('@').map_or(self.host.name(), |(_, name)| name);
        if north_base::platform::orchestration::read().is_ok_and(|config| !probes::takes_sessions(&config, name)) {
            return Vec::new();
        }
        match self.host.run(&probes::launchable_script()) {
            Ok(listing) => probes::PROVIDERS.iter().filter(|provider| listing.lines().any(|line| line.trim() == **provider)).map(|provider| (*provider).to_owned()).collect(),
            Err(_) => probes::PROVIDERS.iter().map(|provider| (*provider).to_owned()).collect(),
        }
    }

    fn sessions_reach_hub(&self, host: &str) -> bool {
        let name = host.rsplit_once('@').map_or(host, |(_, name)| name);
        north_base::platform::orchestration::read().map_or(true, |config| probes::sessions_reach_hub(&config, name))
    }

    fn offloads_to_hub(&self, host: &str) -> bool {
        let name = host.rsplit_once('@').map_or(host, |(_, name)| name);
        north_base::platform::orchestration::read().map_or(true, |config| probes::offloads_to_hub(&config, name))
    }

    fn settles_fleet(&self, _host: &str) -> bool {
        matches!(self.host, super::probe::Shell::Here(_)) && north_offload::land_offload::settles_fleet_here()
    }

    fn takes_land_checks(&self, host: &str) -> bool {
        let name = host.rsplit_once('@').map_or(host, |(_, name)| name);
        north_offload::land_offload::offloads_builds_in(name)
    }

    fn lander_repositories(&self) -> Option<Landers> {
        self.host.run(&probes::repositories_script()).ok().map(|listing| probes::parse_repositories(&listing))
    }

    fn landing_freeze(&self) -> Option<String> {
        self.host.run("cat \"${XDG_STATE_HOME:-$HOME/.local/state}/north/land-freeze.json\" 2>/dev/null; true").ok().filter(|text| !text.trim().is_empty())
    }

    fn lander_take_host(&self, sha: &str) -> Option<String> {
        let live = self.rollback_target(sha)?;
        let mirror = north_trusted_mirror::Mirror::open(&update::checkout()).ok()?;
        take_host(mirror.git_dir(), &live, sha).unwrap_or_else(|why| {
            println!("north runtime canary: {}: no build host take probe: {why}", layout::short(sha));
            None
        })
    }

    fn listed_by(&self, host: &str, sha: &str) -> Result<CoreLoop, String> {
        listing::listed_by_live(self, host, sha)
    }
}

pub(super) fn take_host(mirror: &std::path::Path, live: &str, candidate: &str) -> Result<Option<String>, String> {
    let git = |args: &[&str]| north_trusted_mirror::command(mirror).args(args).output().ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let held = || north_trusted_mirror::command(mirror).args(["cat-file", "-e", &format!("{candidate}^{{commit}}")]).stderr(std::process::Stdio::null()).status().is_ok_and(|status| status.success());
    if !held() && (git(&["fetch", "-q", "--no-tags", "--no-write-fetch-head", "origin"]).is_none() || !held()) {
        return Err(format!("the trusted mirror {} holds no candidate commit {candidate}, even after fetching origin", mirror.display()));
    }
    if git(&["diff", "--name-only", &format!("{live}..{candidate}")]).is_some_and(|changed| !probes::changes_a_take_path(&changed)) {
        return Ok(None);
    }
    let Some(hosts) = git(&["config", "--blob", &format!("{candidate}:.safe-push"), "--get-all", "landing.remote-hosts"]) else {
        return Ok(None);
    };
    Ok(hosts.split(|character: char| character.is_whitespace() || character == ',').find(|host| !host.is_empty()).filter(|host| probes::plain_host(host)).map(str::to_owned))
}

#[cfg(test)]
mod tests;
