//! The dispatch host's disk, last round and last fleet tick, written there and carried to the other hosts by the laptop's records mirror: the time, the root disk's free GiB and whether that closes the host, the sized read of the harness's stores, the last disk keep, the round's counts per reason, the seconds each round phase and tick step took and the seconds since the last round and tick began. It names no issue, box, session or path outside the harness's state and cache roots, because the mirror lands where confined workers read it.
use super::disk_stores::{self, Keep, Sized};
use super::phase;
use super::round::Round;
use super::survey::Survey;
use north_base::platform::records_mirror;
use north_pacing::pacing::placement;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

pub const RECORD: &str = "north/dispatch/host.json";
pub const LIMIT_BYTES: u64 = 64 << 10;
const SIZES_FRESH: f64 = 300.0;
const NAME_CHARS: usize = 64;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Record {
    pub at: f64,
    pub free_gib: Option<f64>,
    pub disk_closed: Option<bool>,
    pub sizes_at: f64,
    pub stores: Vec<Sized>,
    pub last_keep: Option<Keep>,
    pub ready: usize,
    pub started: usize,
    pub not_started: BTreeMap<String, usize>,
    pub since_last: Option<f64>,
    pub seconds: BTreeMap<String, f64>,
    pub tick: Option<Tick>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Tick {
    pub at: f64,
    pub since_last: Option<f64>,
    pub wall: f64,
    pub steps: BTreeMap<String, f64>,
}

/// Writes the record of `round` and this host's disk at `at`, re-sizing the stores only once the last sizing is 5 min old.
pub fn write(survey: &Survey, round: &Round, at: f64) -> Result<(), String> {
    let Some(home) = survey.home.as_deref() else { return Ok(()) };
    let reading = placement::saturation_reading(&survey.here).ok();
    update(&records_mirror::state_home().join(RECORD), |previous| {
        let (sizes_at, stores) = match previous {
            Some(previous) if at - previous.sizes_at < SIZES_FRESH => (previous.sizes_at, previous.stores.clone()),
            _ => (at, disk_stores::sizes(home).into_iter().map(|(size, _)| size).collect()),
        };
        Record {
            at,
            free_gib: reading.as_ref().map(|reading| reading.disk_free_gib),
            disk_closed: reading.as_ref().map(|reading| reading.disk_closed()),
            sizes_at,
            stores,
            last_keep: disk_stores::last_keep(),
            ready: round.ready,
            started: round.started,
            not_started: round.not_started.clone(),
            since_last: previous.filter(|previous| previous.at > 0.0).map(|previous| at - previous.at),
            seconds: phase::seconds(),
            tick: previous.and_then(|previous| previous.tick.clone()),
        }
    })
}

/// Writes the seconds each step of the fleet tick begun at `at` took, its wall seconds and the seconds since the last tick began, keeping the round's part.
pub fn write_tick(at: f64, wall: f64, steps: BTreeMap<String, f64>) -> Result<(), String> {
    update(&records_mirror::state_home().join(RECORD), |previous| {
        let since_last = previous.and_then(|previous| previous.tick.as_ref()).map(|tick| at - tick.at);
        Record { tick: Some(Tick { at, since_last, wall, steps }), ..previous.cloned().unwrap_or_default() }
    })
}

/// The record in `text` parsed and written again with only its known fields, its names cut to plain short ones, or why it is dropped.
pub fn republish(text: &[u8]) -> Result<Vec<u8>, String> {
    if text.len() as u64 > LIMIT_BYTES {
        return Err(format!("{} bytes, over the {LIMIT_BYTES} byte limit", text.len()));
    }
    let mut record: Record = serde_json::from_slice(text).map_err(|error| format!("does not parse: {error}"))?;
    record.stores.retain(|store| plain(&store.name));
    record.last_keep = record.last_keep.filter(|keep| plain(&keep.ran));
    record.not_started.retain(|name, _| plain(name));
    record.seconds.retain(|name, _| plain(name));
    if let Some(tick) = record.tick.as_mut() {
        tick.steps.retain(|name, _| plain(name));
    }
    serde_json::to_vec(&record).map_err(|error| error.to_string())
}

fn plain(name: &str) -> bool {
    name.len() <= NAME_CHARS && name.chars().all(|c| c.is_ascii_alphanumeric() || " _.:-/()".contains(c))
}

fn update(path: &Path, make: impl FnOnce(Option<&Record>) -> Record) -> Result<(), String> {
    let directory = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let lock = File::create(path.with_extension("lock")).map_err(|error| format!("{}: {error}", path.display()))?;
    lock.lock().map_err(|error| format!("{}: {error}", path.display()))?;
    let previous: Option<Record> = std::fs::read(path).ok().and_then(|text| serde_json::from_slice(&text).ok());
    let text = serde_json::to_vec(&make(previous.as_ref())).map_err(|error| error.to_string())?;
    let staged: PathBuf = path.with_extension("json.tmp");
    std::fs::write(&staged, text).and_then(|()| std::fs::rename(&staged, path)).map_err(|error| format!("{}: {error}", path.display()))
}
