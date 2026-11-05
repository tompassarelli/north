//! Placement's projections scored: each launch of the week whose account windows dispatch projected
//! at its ETA, against the used percent each window read at the ETA, as the p50 and p90 of the
//! error in points and its mean sign per window.
use crate::foundation::clock::Timestamp;
use crate::ledger::closed::percentile;
use crate::dispatch::{LaunchRecord as Record, launch_directory, launch_records};
use rusqlite::{Connection, OptionalExtension, params};

/// The windows `north tune` scores, in the order it prints them.
pub const WINDOWS: [&str; 2] = ["5h", "week"];
/// How long after the ETA a reading may come and still stand for what the window read at it.
pub const READING_SLACK_S: f64 = 15.0 * 60.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Scored {
    pub window: String,
    pub projected: f64,
    pub actual: f64,
}

/// Each window of a started launch since `since` whose ETA came by `now` before the window reset,
/// with the reading at the ETA.
pub fn scored(launches: &[Record], since: f64, now: f64, readings: &Ledger) -> Vec<Scored> {
    launches
        .iter()
        .filter(|launch| launch.launched && launch.at >= since)
        .filter_map(|launch| Some((launch, launch.projection.as_ref()?)))
        .filter(|(launch, projection)| launch.at + projection.eta_h * 3600.0 <= now)
        .flat_map(|(launch, projection)| {
            let eta_at = launch.at + projection.eta_h * 3600.0;
            projection.windows.iter().filter(|window| window.at_eta).filter_map(move |window| {
                Some(Scored { window: window.window.clone(), projected: window.projected_pct, actual: readings.at(&launch.account, &window.window, eta_at)? })
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub window: String,
    pub n: usize,
    pub p50: f64,
    pub p90: f64,
    pub bias: f64,
}

pub fn errors(scored: &[Scored]) -> Vec<Error> {
    WINDOWS
        .iter()
        .filter_map(|window| {
            let signed: Vec<f64> = scored.iter().filter(|score| score.window == *window).map(|score| score.projected - score.actual).collect();
            let n = signed.len();
            let mut missed: Vec<f64> = signed.iter().map(|error| error.abs()).collect();
            (n > 0).then(|| Error { window: (*window).to_owned(), n, p50: percentile(&mut missed, 50), p90: percentile(&mut missed, 90), bias: signed.iter().sum::<f64>() / n as f64 })
        })
        .collect()
}

/// One line per window: the projection's error against the reading at the ETA, or that none scored.
pub fn lines(scored: &[Scored]) -> Vec<String> {
    let found = errors(scored);
    WINDOWS
        .iter()
        .map(|window| match found.iter().find(|error| error.window == *window) {
            Some(error) => format!("projection {window} at ETA against actual: error p50 {:.1} p90 {:.1} points, bias {:+.1} (n {})", error.p50, error.p90, error.bias, error.n),
            None => format!("projection {window} at ETA against actual: no launch scored"),
        })
        .collect()
}

const WEEK_S: f64 = 7.0 * 86_400.0;

/// The week's dispatch launches scored against the usage ledger at `THREADS_DB` or its default place.
pub fn live(now: f64) -> Result<Vec<Scored>, String> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or("HOME is not set")?);
    let db = crate::usage::open_ledger(&home, std::env::var_os("THREADS_DB"), &|| {})?;
    Ok(scored(&launch_records(&launch_directory()), now - WEEK_S, now, &Ledger(db)))
}

/// The usage ledger's window readings, the same rows usage-gate reads.
pub struct Ledger(pub Connection);

fn stamp(at: f64) -> String {
    Timestamp::from_unix_seconds_f64(at).iso_micros_utc_offset()
}

impl Ledger {
    pub fn at(&self, account: &str, window: &str, at: f64) -> Option<f64> {
        let (provider, label) = account.split_once(':')?;
        let shape = if window == "5h" { "window_min = 300" } else { "name IN ('week', 'weekly_all')" };
        let sql = format!(
            "SELECT used_pct FROM usage WHERE provider = ?1 AND account = ?2 AND kind = 'window' AND {shape} AND used_pct IS NOT NULL \
             AND julianday(ts) >= julianday(?3) AND julianday(ts) <= julianday(?4) ORDER BY julianday(ts) LIMIT 1"
        );
        self.0.query_row(&sql, params![provider, label, stamp(at), stamp(at + READING_SLACK_S)], |row| row.get(0)).optional().ok().flatten()
    }
}
