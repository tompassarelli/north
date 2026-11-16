use crate::fleet::alarm_stats::{LABELS, compute, labels, render};
use crate::fleet::alarms;
use crate::fleet::registry::root_here;
use crate::foundation::clock::Timestamp;

const USAGE: &str = "Usage: north alarm stats [--since 7d|48h]";

fn window(text: &str) -> Option<i64> {
    let (number, unit) = text.split_at(text.len().checked_sub(1)?);
    let number: i64 = number.parse().ok().filter(|number| *number > 0)?;
    match unit {
        "d" => Some(number * 86_400),
        "h" => Some(number * 3_600),
        _ => None,
    }
}

pub(super) fn run(arguments: &[String]) -> Result<(), String> {
    let since = match arguments {
        [verb] if verb == "stats" => "7d",
        [verb, flag, since] if verb == "stats" && flag == "--since" => since,
        _ => return Err(USAGE.into()),
    };
    let span = window(since).ok_or(USAGE)?;
    let root = root_here().map_err(|error| error.user_message())?;
    let from = Timestamp::now().unix_seconds() - span;
    print!("{}", render(&compute(&alarms::read(&root), &labels(LABELS)?, from), since));
    Ok(())
}
