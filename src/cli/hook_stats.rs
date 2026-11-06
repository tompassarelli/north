use std::path::PathBuf;

use crate::foundation::clock::Timestamp;
use crate::guard::hook_stats::{Fire, PRUNE_DAYS, compute, render};

const USAGE: &str = "Usage: north hook stats [--since 7d|48h]";

fn window(text: &str) -> Option<i64> {
    let (number, unit) = text.split_at(text.len().checked_sub(1)?);
    let number: i64 = number.parse().ok().filter(|number| *number > 0)?;
    match unit {
        "d" => Some(number * 86_400),
        "h" => Some(number * 3_600),
        _ => None,
    }
}

fn arguments(rest: &[String]) -> Result<String, String> {
    match rest {
        [] => Ok("7d".into()),
        [flag, since] if flag == "--since" && window(since).is_some() => Ok(since.clone()),
        _ => Err(USAGE.into()),
    }
}

pub fn run(rest: &[String]) -> Result<(), String> {
    let since = arguments(rest)?;
    let span = window(&since).ok_or(USAGE)?;
    let now = Timestamp::now().unix_seconds();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let explicit = std::env::var_os("THREADS_DB").filter(|path| !path.is_empty());
    let db = crate::usage::open_ledger(&home, explicit, &crate::threads::client::refresh_mirrored_snapshot)?;
    let oldest = Timestamp::from_unix_seconds(now - span.max(PRUNE_DAYS * 86_400)).iso();
    let rows = crate::threads::runs::hooks::load(&db, &oldest)?;
    let fires: Vec<Fire> = rows
        .into_iter()
        .filter_map(|row| Some(Fire { at: Timestamp::parse(&row.at).ok().or_else(|| Timestamp::parse_loose(&row.at))?.unix_seconds(), session: row.session, verdict: row.verdict, rule: row.rule, turn: row.turn }))
        .collect();
    print!("{}", render(&compute(&fires, now - span, now), &since));
    Ok(())
}
