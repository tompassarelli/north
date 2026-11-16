//! The seat probe's log (north#1553): one line for each synced write to the owner's filesystem, and one line each time the breaker stopped the fleet's laptop units or the coordinator resumed them. `north capacity seat-stall` reads it, and placement judges the laptop's seat by it.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::records_mirror::state_home;
use serde::{Deserialize, Serialize};

mod probe;
mod probe_tests;
mod report;

pub use probe::run as probe_run;
pub use report::run;

pub const INTERVAL_MS: u64 = 2_000;
pub const SLOW_MS: u64 = 1_000;
/// Guess: how long a stall keeps placement off the laptop after its synced write returns; the stall log of a week settles it.
const RECENT_MS: u64 = 60_000;
const UNITS: &str = "seat_stall_units";
const SAMPLES: &str = "samples.log";
const SAMPLES_OLDER: &str = "samples.log.1";
const EVENTS: &str = "events.log";
const SAMPLES_BYTES: u64 = 4 << 20;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub at_ms: u64,
    pub sync_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Stop { at_ms: u64, stall_since_ms: u64, units: Vec<String> },
    Resume { at_ms: u64, units: Vec<String> },
}

impl Event {
    fn at_ms(&self) -> u64 {
        match self {
            Self::Stop { at_ms, .. } | Self::Resume { at_ms, .. } => *at_ms,
        }
    }
}

pub fn dir() -> PathBuf {
    state_home().join("north/seat-probe")
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

pub fn trip_ms() -> u64 {
    u64::try_from(crate::capacity::agent_load::seat_stall_trip().as_millis()).unwrap_or(u64::MAX)
}

/// The unit patterns of harness/defaults.toml's `seat_stall_units`, the fleet's units on the laptop the breaker stops.
pub fn unit_patterns() -> Vec<String> {
    document(DEFAULTS)[UNITS]["value"].as_array().into_iter().flatten().filter_map(|pattern| pattern.as_str().map(str::to_owned)).collect()
}

pub struct Log {
    pub samples: Vec<Sample>,
    pub events: Vec<Event>,
}

impl Log {
    /// The probe's log in `dir`, None when the probe has written no sample there.
    pub fn read(dir: &Path) -> Result<Option<Self>, String> {
        let lines = |name: &str| match fs::read_to_string(dir.join(name)) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{}: {error}", dir.join(name).display())),
        };
        let (older, newer) = (lines(SAMPLES_OLDER)?, lines(SAMPLES)?);
        if older.is_none() && newer.is_none() {
            return Ok(None);
        }
        let samples = [older, newer].into_iter().flatten().flat_map(|text| text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect::<Vec<Sample>>()).collect();
        let events = lines(EVENTS)?.unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
        Ok(Some(Self { samples, events }))
    }

    /// How long the synced write the probe started after its newest sample has been out, 0 when it is not yet due.
    pub fn pending_ms(&self, now_ms: u64) -> u64 {
        self.samples.last().map_or(0, |last| now_ms.saturating_sub(last.at_ms + last.sync_ms.max(INTERVAL_MS)))
    }

    /// The longest synced write in the last `RECENT_MS`, counting one still out.
    pub fn recent_stall_ms(&self, now_ms: u64) -> u64 {
        let since = now_ms.saturating_sub(RECENT_MS);
        self.samples.iter().filter(|sample| sample.at_ms + sample.sync_ms >= since).map(|sample| sample.sync_ms).fold(self.pending_ms(now_ms), u64::max)
    }

    /// The breaker's newest stop that no resume followed, with the units it stopped.
    pub fn standing_stop(&self) -> Option<&Event> {
        self.events.iter().rev().find(|event| matches!(event, Event::Resume { .. }) || matches!(event, Event::Stop { units, .. } if !units.is_empty())).filter(|event| matches!(event, Event::Stop { .. }))
    }
}

/// The longest synced write of the seat in the last minute, in milliseconds, 0 where no probe writes a log.
pub fn recent_stall_ms() -> u64 {
    Log::read(&dir()).ok().flatten().map_or(0, |log| log.recent_stall_ms(now_ms()))
}

/// Appends `line` to the log file `name` in `dir`, first moving a samples file past `SAMPLES_BYTES` to its older name, so the log keeps at most two.
fn append(dir: &Path, name: &str, line: &str) -> Result<(), String> {
    let path = dir.join(name);
    if name == SAMPLES && fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= SAMPLES_BYTES) {
        fs::rename(&path, dir.join(SAMPLES_OLDER)).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes())).map_err(|error| format!("{}: {error}", path.display()))
}
