//! The replay of a red hosted run's new cases, or of the build or lint step it failed when it failed no case (north#1537): `north runtime trip replay COMMIT` (trip_command.rs), started by the screen on the host
//! that settles the fleet in a scope of its own, reads those cases on a build host at the commit and its parent, twice each, through
//! `north capacity offload run` (north_base::foundation::pair). Only a regression, the parent green twice and the commit red twice
//! the same way, writes the trip; any other answer trips nothing and records each case in unreproduced.tsv. In 58 of 65 red hosted
//! runs from 2026-11-10 to 2026-11-15 the blamed head was not the cause (south red-main-profile-2026-11-15).

use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;

use north_base::foundation::pair::{Compared, Pair, Reading};

use super::scope::own_scope;
use super::layout;
use super::probe;
use super::trip::{Trip, Trips};

const TIMEOUT: &str = "hosted_replay_timeout_sec";
const READS: u64 = 4;
const SPARE_SECONDS: u64 = 600;

/// Starts the replay of `asked`'s cases unless one runs, answering `None` while it runs, or reads them itself and answers the pair.
pub(crate) type Replay = Box<dyn Fn(&Trips, &Trip, u64) -> Result<Option<Compared>, String>>;

/// The replay on this host: `north runtime trip replay COMMIT` in a scope of its own, logged beside the trips, started once per red run unless the one recorded ran longer than its four reads can take; it writes its answer itself (`settle`), so the screen answers nothing yet.
pub(crate) fn here() -> Replay {
    Box::new(|trips, asked, now| {
        let stale_after = READS * timeout() + SPARE_SECONDS;
        if trips.start_replay(asked, now, stale_after)? {
            start(&trips.0, &asked.head).map_err(|error| format!("the replay of {} could not start: {error}", asked.head))?;
        }
        Ok(None)
    })
}

pub(super) fn timeout() -> u64 {
    probe::default_number(TIMEOUT) as u64
}

fn start(dir: &Path, head: &str) -> std::io::Result<()> {
    let log = OpenOptions::new().create(true).append(true).open(dir.join(format!("{head}.replay.log")))?;
    own_scope(std::env::current_exe()?).args(["runtime", "trip", "replay", head]).stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log).process_group(0).spawn().map(drop)
}

/// Writes what the pair `compared` says of the new cases `asked` names: their trip when they regressed, else their unreproduced record, telling the coordinator either way.
pub(crate) fn settle(trips: &Trips, asked: &Trip, compared: &Compared, at: u64, alert: &dyn Fn(&str)) -> Result<Option<Trip>, String> {
    if compared.regressed() {
        trips.write(asked)?;
        alert(&format!("north runtime: {}", asked.refusal()));
        return Ok(Some(asked.clone()));
    }
    let why = format!("{} at {} and {} at its parent on a build host: {}", twice(&compared.candidate), layout::short(&asked.head), twice(&compared.base), outcome(compared.pair));
    unreproduced(trips, asked, &why, at, alert)?;
    Ok(None)
}

pub(super) fn unreproduced(trips: &Trips, asked: &Trip, why: &str, at: u64, alert: &dyn Fn(&str)) -> Result<(), String> {
    trips.write_unreproduced(asked, why, at)?;
    alert(&format!("north runtime: the hosted Buck2 run {} on {} trips nothing: {why}", asked.run.as_deref().unwrap_or_default(), asked.head));
    Ok(())
}

fn twice(runs: &[Reading; 2]) -> String {
    if runs[0] == runs[1] { format!("{} twice", runs[0].word()) } else { format!("{} then {}", runs[0].word(), runs[1].word()) }
}

fn outcome(pair: Pair) -> &'static str {
    match pair {
        Pair::Regressed => "a regression",
        Pair::Fixed => "red at the parent and green at the commit",
        Pair::SameRed => "red at both the same way",
        Pair::RedChanged => "red at both, another way at the commit",
        Pair::SameGreen => "green at both, so the hosted runner's red does not fail again",
        Pair::NoVerdict => "no verdict",
        Pair::TwoAnswers => "two answers at one commit, so the check is nondeterministic and blames no commit",
    }
}

/// A replay that reads every asked case green at the parent and red at the commit, so each red run it is asked of trips.
#[cfg(test)]
pub(crate) fn regressing() -> Replay {
    Box::new(|_, asked, _| Ok(Some(north_base::foundation::pair::compare(&|commit: &str| if commit == asked.head { Reading::Red("failed".to_owned()) } else { Reading::Green("passed".to_owned()) }, "parent", asked.head.as_str()))))
}
