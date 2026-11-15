//! `north ledger duplicate`: the worker runs of the last days that started on a box another worker run
//! was still working, so a day of duplicate stagings reads as a count and a day without one reads 0.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};

const USAGE: &str = "usage: north ledger duplicate [--days N] [--json]";
const DAY_S: i64 = 86_400;

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, args_override_self = true)]
pub(super) struct Options {
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    pub(super) days: u32,
    #[arg(long)]
    pub(super) json: bool,
}

pub(super) fn parse(arguments: &[String]) -> Option<Options> {
    north_foundation::foundation::arguments::parse(arguments).ok()
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
