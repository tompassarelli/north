//! `north ledger duplicate`: the worker runs of the last days that started on a box another worker run
//! was still working, so a day of duplicate stagings reads as a count and a day without one reads 0.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};

const USAGE: &str = "usage: north ledger duplicate [--days N] [--json]";
const DAY_S: i64 = 86_400;

struct Options {
    days: u32,
    json: bool,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { days: 1, json: false };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--days" => options.days = words.next()?.parse().ok().filter(|days| *days > 0)?,
            "--json" => options.json = true,
            _ => return None,
        }
    }
    Some(options)
}

/// The duplicate starts of each day from `since` on, keyed by day: a worker run counts once for each day it
/// started while another worker run on the same item and box was still running.
pub fn read(database: &Path, since: &str) -> Result<BTreeMap<String, usize>, String> {
    let query = format!(
        "SELECT substr(b.started, 1, 10) AS day, count(DISTINCT b.agent) AS duplicates \
         FROM runs a JOIN runs b ON a.item = b.item AND coalesce(a.box, '') = coalesce(b.box, '') \
         AND a.role = 'worker' AND b.role = 'worker' AND coalesce(a.box, '') != '' AND a.agent != b.agent \
         AND b.started > a.started AND b.started < a.ended \
         WHERE a.outcome != 'running' AND coalesce(a.ended, '') != '' AND b.started >= '{}' \
         GROUP BY day ORDER BY day",
        since.replace('\'', "")
    );
    let rows = sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), &query).map_err(|error| error.to_string())?;
    Ok(rows.iter().filter_map(|row| Some((row["day"].as_str()?.to_owned(), usize::try_from(row["duplicates"].as_i64()?).ok()?))).collect())
}

/// The days `--days` covers, oldest first, each with its duplicate count, zero when none.
pub fn days(counts: &BTreeMap<String, usize>, now: Timestamp, days: u32) -> Vec<(String, usize)> {
    let seconds = now.unix_seconds();
    (0..i64::from(days))
        .rev()
        .map(|back| {
            let day = Timestamp::from_unix_seconds(seconds - back * DAY_S).iso_date();
            let count = counts.get(&day).copied().unwrap_or(0);
            (day, count)
        })
        .collect()
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let now = Timestamp::now();
    let since = Timestamp::from_unix_seconds(now.unix_seconds() - i64::from(options.days - 1) * DAY_S).iso_date();
    let counts = match read(&crate::ledger::threads_database(), &since) {
        Ok(counts) => counts,
        Err(why) => {
            eprintln!("north ledger duplicate: {why}");
            return 1;
        }
    };
    let days = days(&counts, now, options.days);
    if options.json {
        let rows: Vec<Value> = days.iter().map(|(day, count)| json!({"day": day, "duplicates": count})).collect();
        println!("{}", json!({"days": rows}));
    } else {
        for (day, count) in days {
            println!("duplicate stagings {day}: {count}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(rows: &[(&str, &str, &str, &str, &str)]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("threads.db");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, box TEXT, role TEXT, started TEXT, ended TEXT, outcome TEXT)").unwrap();
        for (agent, item, box_text, started, ended) in rows {
            connection.execute("INSERT INTO runs(agent, item, box, role, started, ended, outcome) VALUES (?1, ?2, ?3, 'worker', ?4, ?5, 'done')", rusqlite::params![agent, item, box_text, started, ended]).unwrap();
        }
        directory
    }

    #[test]
    fn a_run_that_started_while_another_worked_its_box_counts_once_and_a_day_without_one_reads_zero() {
        let directory = database(&[
            ("north-634-d2105", "north#634", "gex45 placement fits its build capacity", "2026-11-07T21:05:59Z", "2026-11-07T21:10:00Z"),
            ("north-634-d2051", "north#634", "gex45 placement fits its build capacity", "2026-11-07T21:06:24Z", "2026-11-07T21:10:00Z"),
            ("north-634-d2109", "north#634", "gex45 placement fits its build capacity", "2026-11-07T21:09:00Z", "2026-11-07T21:15:00Z"),
            ("north-634-d2359", "north#634", "gex45 placement fits its build capacity", "2026-11-07T23:59:00Z", "2026-11-08T00:20:00Z"),
            ("north-634-later", "north#634", "Replay never holds the lander for long", "2026-11-07T21:07:00Z", "2026-11-07T21:20:00Z"),
            ("north-635-d2110", "north#635", "a next box", "2026-11-07T21:10:00Z", "2026-11-07T21:20:00Z"),
        ]);
        let counts = read(&directory.path().join("threads.db"), "2026-11-07").unwrap();
        assert_eq!(counts.get("2026-11-07"), Some(&2), "the two runs that started on the worked box count once each, and no other box counted");
        assert_eq!(days(&counts, Timestamp::parse("2026-11-09T12:00:00Z").unwrap(), 1), [("2026-11-09".to_owned(), 0)], "a day without a duplicate staging reads zero");
    }
}
