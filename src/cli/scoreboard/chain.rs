//! A first try's automatic chain: a failed run the harness resumed by itself (a `resume` cause row, north#790) or
//! relaunched on another host (a `host-need` cause row from `north fleet need`, then `Follows: SESSION (Stop: host NEED)`) is one first try, judged where the chain ends. A
//! message from Tom, a lead or the proxy (a `tom` or `proxy` cause row) ends the chain: the run stays the failed first try.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

/// The `source` a boundary writes on the cause row of an automatic resume; `recorded_column` skips it.
const RESUME_SOURCE: &str = "resume";
/// The `source` `north fleet need` writes on the cause row of a host need a worker recorded.
const NEED_SOURCE: &str = "host-need";

/// How a run ended: what the scorer reads of a first try.
#[derive(Clone)]
pub struct End {
    pub outcome: String,
    pub recorded: Option<String>,
}

#[derive(Clone)]
pub struct Raw {
    pub agent: String,
    pub category: String,
    pub item: String,
    pub session: String,
    pub started: i64,
    pub ended: i64,
    pub end: End,
}

/// The cause rows boundaries wrote per fleet session, as unix seconds.
#[derive(Default)]
pub struct Marks {
    resumes: BTreeMap<String, Vec<i64>>,
    corrections: BTreeMap<String, Vec<i64>>,
    needs: BTreeMap<String, Vec<i64>>,
}

impl Marks {
    pub fn read(db: &Connection) -> Self {
        let mut marks = Self::default();
        let Ok(mut statement) = db.prepare("SELECT session, source, cause, at FROM boundary_causes") else { return marks };
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)));
        for (session, source, cause, at) in rows.into_iter().flatten().flatten() {
            let Ok(at) = Timestamp::parse(&at).map(|at| at.unix_seconds()) else { continue };
            let kind = if source == RESUME_SOURCE { &mut marks.resumes } else if source == NEED_SOURCE { &mut marks.needs } else if matches!(cause.as_str(), "tom" | "proxy") { &mut marks.corrections } else { continue };
            kind.entry(session).or_default().push(at);
        }
        marks
    }

    fn any(marks: &BTreeMap<String, Vec<i64>>, session: &str, after: i64, before: i64) -> bool {
        marks.get(session).is_some_and(|ats| ats.iter().any(|at| *at >= after && *at < before))
    }
}

fn settled(end: &End) -> bool {
    !matches!(end.outcome.as_str(), "running" | "idle")
}

/// `rows` with each chain folded into its first run, which takes the end of the chain; a chain not yet ended is left
/// out. `relaunches` maps a session that recorded a host need to the end of the run that relaunched it; a run whose session
/// recorded a host need inside its window and was not yet relaunched is a chain not yet ended while it ended after
/// `relaunch_since`, and its own end after that.
pub fn fold(mut rows: Vec<Raw>, marks: &Marks, relaunches: &BTreeMap<String, End>, relaunch_since: i64) -> Vec<Raw> {
    rows.sort_by_key(|row| row.started);
    let mut dropped = BTreeSet::new();
    let mut by_session: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (at, row) in rows.iter().enumerate().filter(|(_, row)| !row.session.is_empty()) {
        by_session.entry(&row.session).or_default().push(at);
    }
    let mut ends: BTreeMap<usize, End> = BTreeMap::new();
    for (session, indexes) in &by_session {
        let mut at = 0;
        while at < indexes.len() {
            let (head, mut last, mut pending) = (indexes[at], indexes[at], false);
            at += 1;
            loop {
                let before = indexes.get(at).map_or(i64::MAX, |next| rows[*next].ended);
                if !Marks::any(&marks.resumes, session, rows[last].ended, before) || Marks::any(&marks.corrections, session, rows[last].ended, before) {
                    break;
                }
                let Some(next) = indexes.get(at).copied() else {
                    pending = true;
                    break;
                };
                dropped.insert(next);
                last = next;
                at += 1;
            }
            if pending {
                dropped.insert(head);
            } else if last != head {
                ends.insert(head, rows[last].end.clone());
            }
        }
    }
    for (at, row) in rows.iter().enumerate() {
        if let Some(relaunch) = relaunches.get(&row.session).filter(|_| !ends.contains_key(&at) && !dropped.contains(&at)) {
            if settled(relaunch) {
                ends.insert(at, relaunch.clone());
            } else {
                dropped.insert(at);
            }
        } else if row.ended >= relaunch_since && !ends.contains_key(&at) && Marks::any(&marks.needs, &row.session, row.started, row.ended.saturating_add(1)) {
            dropped.insert(at);
        }
    }
    rows.into_iter().enumerate().filter(|(at, _)| !dropped.contains(at)).map(|(at, row)| match ends.remove(&at) {
        Some(end) => Raw { end, ..row },
        None => row,
    }).collect()
}
