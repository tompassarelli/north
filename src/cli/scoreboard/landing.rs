use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::Connection;

use super::{Better, Figure, Measure, Window};
use crate::foundation::clock::Timestamp;
use crate::land::trace::profile::Trace;
use crate::land::trace::report::every_queue;
use crate::land::trace::Spans;
use crate::ledger::outcome::attention::references;
use crate::threads::calibration::rework::{REWORK_WINDOW_S, Rework, Sources};

fn seconds(text: &str) -> Option<i64> {
    Timestamp::parse(text).ok().map(Timestamp::unix_seconds)
}

/// When each issue's worker runs reached main, ascending, by `repo#N`.
pub fn first_landings(db: &Connection) -> rusqlite::Result<HashMap<String, Vec<i64>>> {
    let mut statement = db.prepare("SELECT item, landed_at FROM runs WHERE coalesce(item, '') != '' AND coalesce(landed_at, '') != ''")?;
    let mut found: HashMap<String, Vec<i64>> = HashMap::new();
    for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
        let (item, landed) = row?;
        let Some(at) = seconds(&landed) else { continue };
        for name in references(&item) {
            found.entry(name).or_default().push(at);
        }
    }
    found.values_mut().for_each(|times| times.sort_unstable());
    Ok(found)
}

pub fn queue_to_main(db: &Connection, weeks: [Window; 2]) -> rusqlite::Result<Measure> {
    let mut statement = db.prepare("SELECT queued_at, landed_at FROM landings WHERE verdict = 'landed' AND coalesce(landed_at, '') != ''")?;
    let landed: Vec<(i64, i64)> =
        statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.filter_map(Result::ok).filter_map(|(queued, landed)| Some((seconds(&queued)?, seconds(&landed)?))).collect();
    let figure = |window: Window| Figure::Spread { seconds: landed.iter().filter(|(_, at)| window.holds(*at)).map(|(queued, at)| (at - queued).max(0) as f64).collect(), open: 0, open_label: "" };
    Ok(Measure { name: "landing queue to main".into(), better: Better::Lower, source: "the lander's landings ledger, queued_at to landed_at of each landed lane (threads landings)", weeks: weeks.map(figure) })
}

pub fn verification(weeks: [Window; 2]) -> Measure {
    let spans: Vec<_> = every_queue().iter().flat_map(|file| Spans::at(file).read()).collect();
    let trace = Trace::of(&spans);
    let profiles: Vec<(f64, f64)> = trace.landings(weeks[1].start as f64).into_iter().map(|landing| (landing.end, trace.profile(landing).after_wall)).filter(|(_, wall)| *wall > 0.0).collect();
    let figure = |window: Window| Figure::Spread { seconds: profiles.iter().filter(|(end, _)| window.holds(*end as i64)).map(|(_, wall)| *wall).collect(), open: 0, open_label: "" };
    Measure { name: "verification after the push".into(), better: Better::Lower, source: "each landing's after-push check spans in the land queues' spans.jsonl, as north land profile reads them", weeks: weeks.map(figure) }
}

/// One worker landing on main, judged for rework, with the repositories its run's item names.
pub struct Judged {
    pub at: i64,
    pub reworked: bool,
    pub repos: BTreeSet<String>,
}

pub fn judged_landings(db: &Connection, sources: &Sources) -> Result<Vec<Judged>, String> {
    let judged = Rework::read(db, sources).map_err(|why| format!("rework: {why}"))?.judged;
    let mut statement = db.prepare("SELECT agent, landed_sha, coalesce(item, '') FROM runs WHERE coalesce(landed_sha, '') != ''").map_err(|why| why.to_string())?;
    let shas: HashMap<String, (String, BTreeSet<String>)> =
        statement.query_map([], |row| Ok((row.get::<_, String>(0)?, (row.get::<_, String>(1)?, row.get::<_, String>(2)?)))).map_err(|why| why.to_string())?.filter_map(Result::ok).map(|(agent, (sha, item))| (agent, (sha, super::workers::repos(&item)))).collect();
    let mut landings: BTreeMap<&str, Judged> = BTreeMap::new();
    for (agent, verdict) in &judged {
        let Some((sha, repos)) = shas.get(agent) else { continue };
        let entry = landings.entry(sha.as_str()).or_insert_with(|| Judged { at: verdict.at, reworked: false, repos: repos.clone() });
        entry.reworked |= verdict.cause.is_some();
    }
    Ok(landings.into_values().collect())
}

pub fn rework(db: &Connection, sources: &Sources, weeks: [Window; 2], now: i64) -> Result<Measure, String> {
    let landings = judged_landings(db, sources)?;
    let figure = |window: Window| {
        let held: Vec<&Judged> = landings.iter().filter(|landing| window.holds(landing.at)).collect();
        Figure::Share {
            hits: held.iter().filter(|landing| landing.reworked).count(),
            of: held.len(),
            pending: held.iter().filter(|landing| !landing.reworked && now - landing.at < REWORK_WINDOW_S).count(),
            pending_label: "still inside 48 h",
        }
    };
    Ok(Measure {
        name: "rework within 48 h per landing".into(),
        better: Better::Lower,
        source: "worker landings on each ~/code/<repo>/main reverted, fixed forward or named a red-main culprit within 48 h (threads calibration rework)",
        weeks: weeks.map(figure),
    })
}
