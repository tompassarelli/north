//! The after-landing runs this lander journals in the lander host's lifecycle store, each committed with its head's
//! move to Journaled, and the files a lander from before north#862 left under the queue (north#862).
use std::fs;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{AfterLandingRun, LandedLane, Lander, Run, lifecycle, state};

const JOURNAL: &str = "following";

#[derive(Serialize, Deserialize)]
pub(super) struct Entry {
    pub(super) base: String,
    pub(super) head: String,
    pub(super) lanes: Vec<LandedLane>,
}

pub(super) type Journaled = (f64, Entry);

/// Journals `entry`'s after-landing run in the lander host's lifecycle store with its move to Journaled.
pub(super) fn journal(lander: &Lander, entry: &Entry) -> Result<(), String> {
    let run = serde_json::to_value(entry).map_err(|error| error.to_string())?;
    lifecycle::journal(&mut lifecycle::host()?, &lander.queue, &entry.head, &format!("landed on {}", entry.base), run)
}

/// Every journaled after-landing run of this queue no live lander holds, now held by this one; an unreadable one is
/// retired.
pub(super) fn take_journaled(lander: &Lander) -> Result<Vec<Journaled>, String> {
    let mut store = lifecycle::host()?;
    let mut journaled = Vec::new();
    for leased in lifecycle::take(&mut store, &lander.queue)? {
        match serde_json::from_value::<Entry>(leased.effect.payload) {
            Ok(entry) => journaled.push((Duration::from_millis(leased.queued.unix_millis().try_into().unwrap_or(0)).as_secs_f64(), entry)),
            Err(error) => {
                println!("north land: dropping the unreadable after-landing run of {}: {error}", leased.instance);
                lifecycle::moved::<AfterLandingRun>(&mut store, &leased.instance, Run::Retired, &format!("its journaled run is unreadable: {error}"));
                lifecycle::settled(&mut store, &lander.queue, &leased.instance);
            }
        }
    }
    journaled.sort_by(|left, right| left.0.total_cmp(&right.0));
    Ok(journaled)
}

/// Journals in the lifecycle store each entry a lander from before north#862 left as a file under the queue, then
/// removes the file.
pub(super) fn adopt_journal_files(lander: &Lander) {
    let Ok(files) = fs::read_dir(lander.queue.join(JOURNAL)) else { return };
    for path in files.filter_map(Result::ok).map(|file| file.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")) {
        let entry = fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|text| serde_json::from_str::<Entry>(&text).map_err(|error| error.to_string()));
        let adopted = match entry {
            Ok(entry) if state(&entry.head) != Some(Run::Retired) => journal(lander, &entry),
            _ => Ok(()),
        };
        match adopted {
            Ok(()) => drop(fs::remove_file(&path)),
            Err(error) => println!("north land: the after-landing journal entry {} stays a file: {error}", path.display()),
        }
    }
    let _ = fs::remove_dir(lander.queue.join(JOURNAL));
}
