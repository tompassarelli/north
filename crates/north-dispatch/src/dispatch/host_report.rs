//! The dispatch host's disk and last round, written each real round where the records mirror carries it to the other hosts: the time, the root disk's free GiB and whether that closes the host, the sized read of the harness's stores, the last disk keep and the round's counts per reason. It names no issue, box, session or path outside the harness's state and cache roots, because the mirror lands where confined workers read it.
use super::disk_stores::{self, Keep, Sized};
use super::round::Round;
use super::survey::Survey;
use north_base::platform::records_mirror;
use north_pacing::pacing::placement;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const RECORD: &str = "north/dispatch/host.json";
const SIZES_FRESH: f64 = 300.0;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
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
}

/// Writes the record of `round` and this host's disk at `at`, re-sizing the stores only once the last sizing is 5 min old.
pub fn write(survey: &Survey, round: &Round, at: f64) -> Result<(), String> {
    let Some(home) = survey.home.as_deref() else { return Ok(()) };
    let path = records_mirror::state_home().join(RECORD);
    let previous: Option<Record> = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok());
    let (sizes_at, stores) = match previous {
        Some(previous) if at - previous.sizes_at < SIZES_FRESH => (previous.sizes_at, previous.stores),
        _ => (at, disk_stores::sizes(home).into_iter().map(|(size, _)| size).collect()),
    };
    let reading = placement::saturation_reading(&survey.here).ok();
    let record = Record {
        at,
        free_gib: reading.as_ref().map(|reading| reading.disk_free_gib),
        disk_closed: reading.as_ref().map(|reading| reading.disk_closed()),
        sizes_at,
        stores,
        last_keep: disk_stores::last_keep(),
        ready: round.ready,
        started: round.started,
        not_started: round.not_started.clone(),
    };
    let text = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    let directory = path.parent().unwrap_or(Path::new("."));
    let staged = path.with_extension("json.tmp");
    std::fs::create_dir_all(directory).and_then(|()| std::fs::write(&staged, text)).and_then(|()| std::fs::rename(&staged, &path)).map_err(|error| format!("{}: {error}", path.display()))
}
