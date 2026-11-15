//! `north runtime trip clear COMMIT --why TEXT [--issue REPO#N]` clears the open Buck2 trip on a commit, the second clear of one failing test within a day naming its class-fix issue (north#1577), and `north runtime trip replay COMMIT` reads the new cases of its red hosted run again on a build host (replay.rs), both on the host that settles the fleet.

use std::path::Path;

use north_foundation::foundation::exit::NO_VERDICT_STATUS;
use north_foundation::foundation::pair::{self, Reading};
use north_offload::land_offload;
use north_unblocks::{Admission, Kind, Ledger};

use north_runtime_records::failure::{Failure, failed};
use super::main_runs;
use super::order::orderer;
use super::replay;
use north_runtime_records::hold_ledger::{full_sha, git, now};
use super::trip::{Trip, Trips};
use super::update;

type Red = fn(&str) -> Vec<String>;

const STEP: &str = "hosted step";
const BUILD: &str = "./bin/land-shell --dev -- buck2 build --remote-only --materializations=none //...";
const CLIPPY: &str = "./bin/land-shell --dev -- buck2 bxl --remote-only --materializations=none //lint/clippy.bxl:clippy -- --targets //...";
const STEPS: [(&str, &str); 3] = [("Test", BUILD), ("Build north for the harness tests", BUILD), ("Clippy", CLIPPY)];

const USAGE: &str = "usage: north runtime trip clear COMMIT --why TEXT [--issue REPO#N] | north runtime trip replay COMMIT  clear the open Buck2 trip that names COMMIT, so the promotion of that head goes live again; the clear keeps who cleared it and why; the second clear of the same failing tests within 24 h is refused unless --issue names an open issue with an owner that fixes their cause; or read the cases the red hosted run of COMMIT failed that the last verified head before it did not, or for a run that failed no case the build or lint step it failed, twice at COMMIT and twice at its parent on a build host, and trip COMMIT only when its parent passes both and COMMIT fails both the same way, else record each case in unreproduced.tsv (the screen starts it); run it on the host that settles the fleet";

pub(super) fn command(flags: &[String], settles: bool) -> Result<(), Failure> {
    let (commit, why, issue) = match flags {
        [verb, commit] if verb == "replay" && full_sha(commit) => (commit, None, None),
        [verb, commit, flag, why] if verb == "clear" && flag == "--why" => (commit, Some(why), None),
        [verb, commit, flag, why, named, issue] if verb == "clear" && flag == "--why" && named == "--issue" => (commit, Some(why), Some(issue.as_str())),
        _ => return Err(failed(64, USAGE)),
    };
    if why.is_some_and(|why| why.trim().is_empty()) {
        return Err(failed(64, USAGE));
    }
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet keeps the trips its order and promotions screen by, and this is not it\n{USAGE}")));
    }
    let Some(why) = why else { return run(commit, now()) };
    let head = git(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")]).ok().filter(|head| full_sha(head)).ok_or_else(|| failed(1, format!("{commit} names no commit here")))?;
    let admission = Admission { ledger: &Ledger::here(), issues: &north_unblocks::GitHub, now: now() };
    let line = clear(&Trips::here(), &admission, &head, &orderer(), why.trim(), issue).map_err(|why| failed(1, why))?;
    println!("{line}");
    Ok(())
}

/// The class a trip clear unblocks: the failed cases it names, or the run when its log named none.
pub fn class(trip: &Trip) -> String {
    let mut cases: Vec<String> = trip.cases.iter().map(|(name, case)| format!("{case} in {name}")).collect();
    cases.sort();
    if cases.is_empty() { "a hosted run whose log names no failed test".to_owned() } else { cases.join(", ") }
}

/// Clears the open trip on `head` once the unblock ledger admits it: the second clear of one class within a day names an open, owned issue.
pub fn clear(trips: &Trips, admission: &Admission, head: &str, by: &str, why: &str, issue: Option<&str>) -> Result<String, String> {
    let trip = trips.read(head)?.ok_or_else(|| format!("no Buck2 trip stands on {head}"))?;
    if trips.cleared(head)? {
        return Err(format!("the Buck2 trip on {head} is already cleared"));
    }
    admission.admit(Kind::TripClear, &class(&trip), by, issue)?;
    trips.clear(head, by, why, admission.now)
}

/// Reads what the replay of `head` names at `head` and its parent on the first build host, from the north checkout, and writes the trip or the unreproduced record; a replay that fails ends its record too, so the next screen starts it again.
fn run(head: &str, now: u64) -> Result<(), Failure> {
    let trips = Trips::here();
    let asked = trips.replaying(head).map_err(|why| failed(1, why))?.ok_or_else(|| failed(1, format!("no replay of {head} is asked")))?;
    let settled = replayed(&trips, &asked, &update::checkout(), now);
    trips.finish_replay(head).map_err(|why| failed(1, why))?;
    match settled.map_err(|why| failed(1, why))? {
        Some(trip) => println!("north runtime trip replay: {}", trip.refusal()),
        None => println!("north runtime trip replay: {head} trips nothing; what it failed is recorded in unreproduced.tsv"),
    }
    Ok(())
}

fn replayed(trips: &Trips, asked: &Trip, checkout: &Path, now: u64) -> Result<Option<Trip>, String> {
    let alert = |text: &str| super::alert::alert_coordinator(text);
    let (asked, command, red, named) = match asked.cases.is_empty() {
        true => {
            let steps = failed_steps(asked)?;
            (Trip { cases: steps.iter().map(|step| (STEP.to_owned(), step.clone())).collect(), ..asked.clone() }, step_command(&steps), errors as Red, Vec::new())
        }
        false => (asked.clone(), nextest(&asked.cases), cases as Red, asked.cases.iter().map(|(name, case)| format!("{case} in {name}")).collect::<Vec<String>>()),
    };
    let Some(command) = command else {
        replay::unreproduced(trips, &asked, "it names no case or step a slot can run by name", now, &alert)?;
        return Ok(None);
    };
    let host = land_offload::build_hosts().into_iter().map(|(name, _)| name).next().ok_or("this host reaches no build host to replay the cases on")?;
    let parent = update::git(checkout, &["rev-parse", "--verify", &format!("{}^", asked.head)]).map_err(|failure| failure.message)?;
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let seconds = replay::timeout();
    let check = |commit: &str| reading(&north, checkout, &host, commit, &command, seconds, (red, &named));
    let compared = pair::compare(&check, parent.as_str(), asked.head.as_str());
    replay::settle(trips, &asked, &compared, now, &alert)
}

/// The steps the red run `asked` names failed, read from its log again, since a run that fails no case was failed by a step before or after its cases: a build or the lint.
fn failed_steps(asked: &Trip) -> Result<Vec<String>, String> {
    let run = asked.run.as_deref().ok_or("the replay names no hosted run")?;
    match (main_runs::MainRuns::main().kept)(run)? {
        main_runs::Kept::Log(text) => Ok(main_runs::failed_steps(&text)),
        main_runs::Kept::CutOff(_) => Ok(Vec::new()),
    }
}

/// The slot line that does again what each of the hosted run's failed `steps` did on main (.github/workflows/buck2.yml) through Buck2 on a build host, or none when a step is not one of them.
fn step_command(steps: &[String]) -> Option<String> {
    let mut commands: Vec<&str> = Vec::new();
    for step in steps {
        let command = STEPS.iter().find(|(name, _)| name == step)?.1;
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
    (!commands.is_empty()).then(|| commands.join(" && "))
}

/// The nextest line that runs exactly `cases` in a slot, or none when a crate or case name holds a character a shell or filter could read as more than a name, since the names come from a hosted run's log.
fn nextest(cases: &[(String, String)]) -> Option<String> {
    let crate_name = |name: &str| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    let case_name = |name: &str| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':'));
    if cases.is_empty() || !cases.iter().all(|(name, case)| crate_name(name) && case_name(case)) {
        return None;
    }
    let mut packages: Vec<&str> = cases.iter().map(|(name, _)| name.as_str()).collect();
    packages.sort_unstable();
    packages.dedup();
    let packages: Vec<String> = packages.into_iter().map(|name| format!("-p {name}")).collect();
    let filter: Vec<String> = cases.iter().map(|(name, case)| format!("(package(={name}) & test(={case}))")).collect();
    Some(format!("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline --no-tests=pass --no-fail-fast {} -E '{}'", packages.join(" "), filter.join(" | ")))
}

/// The cases a nextest run's output failed, each as `CASE in CRATE`.
fn cases(text: &str) -> Vec<String> {
    main_runs::failed_cases(text).iter().map(|(name, case)| format!("{case} in {name}")).collect()
}

/// The compiler and lint errors a build's output names, once each in order of their text.
fn errors(text: &str) -> Vec<String> {
    let mut errors: Vec<String> = text.lines().map(str::trim).filter(|line| line.starts_with("error[") || line.starts_with("error: ")).map(str::to_owned).collect();
    errors.sort_unstable();
    errors.dedup();
    errors
}

/// What one slot run of `command` at `commit` on `host` says, the run tried again while the offload connection is lost before it answers.
fn reading(north: &Path, checkout: &Path, host: &str, commit: &str, command: &str, seconds: u64, (red, named): (Red, &[String])) -> Reading {
    let seconds = seconds.to_string();
    let words = ["capacity", "offload", "run", "--host", host, "--base", commit, "--head", commit, "--timeout", seconds.as_str(), "--", command];
    retried(|| match north_base::platform::process::run(&north.to_string_lossy(), Some(checkout), &words) {
        Ok(output) => Ran::Ended(output.status.code(), format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))),
        Err(error) => Ran::Unstarted(format!("north capacity offload run did not start: {error}")),
    }, red, named)
}

/// How many times one slot run is tried while the offload connection ends before the remote host reports its exit: a guess, settled by how often a third try still loses it.
const TRIES: usize = 3;
/// The exit cargo-nextest ends a run on when a test failed.
const NEXTEST_FAILED: i32 = 100;

pub(super) enum Ran {
    Ended(Option<i32>, String),
    Unstarted(String),
}

/// What a slot run says, tried up to `TRIES` times while it ends with the offload's no-verdict exit: green on exit 0, red with what
/// `red` reads it failed, or with the `named` cases a nextest run of exactly them exited 100 on when the slot kept none of its output, else no verdict with why.
pub(super) fn retried(mut run: impl FnMut() -> Ran, red: Red, named: &[String]) -> Reading {
    let mut ran = run();
    for _ in 1..TRIES {
        if !matches!(ran, Ran::Ended(Some(NO_VERDICT_STATUS), _)) {
            break;
        }
        ran = run();
    }
    let (status, text) = match ran {
        Ran::Ended(status, text) => (status, text),
        Ran::Unstarted(why) => return Reading::NoVerdict(why),
    };
    if status == Some(0) {
        return Reading::Green("it passed".to_owned());
    }
    let mut failures = red(&text);
    if failures.is_empty() && status == Some(NEXTEST_FAILED) {
        failures = named.to_vec();
    }
    if failures.is_empty() {
        let last = text.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default();
        return Reading::NoVerdict(format!("the slot run ended {status:?} and named no failure: {last}"));
    }
    Reading::Red(failures.join(", "))
}

#[cfg(test)]
#[path = "trip_command_tests.rs"]
mod tests;
