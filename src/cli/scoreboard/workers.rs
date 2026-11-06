use std::collections::{BTreeMap, HashSet};

use rusqlite::Connection;

use super::{Better, Figure, Measure, Window};
use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::worker_where;

const SOURCE: &str = "worker-ledger runs with no Follows: (first attempts), done with no later run following them, split, idle and running runs left out (threads runs)";
const ALL: &str = "all categories";

struct Attempt {
    category: String,
    ended: i64,
    succeeded: bool,
}

fn attempts(db: &Connection, since: i64) -> rusqlite::Result<Vec<Attempt>> {
    let mut statement = db.prepare("SELECT DISTINCT follows FROM runs WHERE coalesce(follows, '') != ''")?;
    let followed: HashSet<String> = statement.query_map([], |row| row.get(0))?.filter_map(Result::ok).collect();
    let mut statement = db.prepare(&format!(
        "SELECT agent, category, ended, outcome FROM runs WHERE {} AND coalesce(follows, '') = '' AND outcome NOT IN ('running', 'idle', 'split') AND ended >= ?1",
        worker_where()
    ))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?;
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|(agent, category, ended, outcome)| {
            let ended = Timestamp::parse(&ended).ok()?.unix_seconds();
            Some(Attempt { succeeded: outcome == "done" && !followed.contains(&agent), category, ended })
        })
        .collect())
}

pub fn first_try(db: &Connection, weeks: [Window; 2]) -> rusqlite::Result<Vec<Measure>> {
    let attempts = attempts(db, weeks[1].start)?;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for attempt in attempts.iter().filter(|attempt| weeks[0].holds(attempt.ended)) {
        *counts.entry(attempt.category.as_str()).or_default() += 1;
    }
    let mut categories: Vec<&str> = attempts.iter().map(|attempt| attempt.category.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    categories.sort_by_key(|category| std::cmp::Reverse(counts.get(category).copied().unwrap_or(0)));
    let figure = |category: &str, window: Window| {
        let held: Vec<&Attempt> = attempts.iter().filter(|attempt| window.holds(attempt.ended) && (category == ALL || attempt.category == category)).collect();
        Figure::Share { hits: held.iter().filter(|attempt| attempt.succeeded).count(), of: held.len(), pending: 0, pending_label: "" }
    };
    Ok(std::iter::once(ALL)
        .chain(categories)
        .map(|category| Measure { name: format!("worker first-try success, {category}"), better: Better::Higher, source: SOURCE, weeks: weeks.map(|window| figure(category, window)) })
        .collect())
}
