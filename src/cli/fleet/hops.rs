//! `north fleet hops NAME...`: where each named session's starts spent their time, read from the
//! launch trace its own processes wrote, one line per start and the medians across them.
use crate::error::{NorthError, NorthResult};
use crate::fleet::hops::{Launch, launches, trace_in};
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;

pub(super) fn run(fleet: &Fleet, names: &[&str]) -> NorthResult<()> {
    if names.is_empty() {
        return Err(NorthError::Usage("Usage: north fleet hops NAME...".into()));
    }
    let mut all = Vec::new();
    for name in names {
        let dir = match fleet.session_dir(name)? {
            dir if dir.exists() => dir,
            _ => fleet.retired_session(name).map(|(dir, _)| dir).ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?,
        };
        let found = launches(&trace_in(&dir));
        if found.is_empty() {
            println!("{name}: no launch trace");
        }
        for launch in &found {
            println!("{}", line(launch));
        }
        all.extend(found);
    }
    if all.len() > 1 {
        println!("median of {} starts: {}", all.len(), offsets(median(&all, |launch| Some(launch.session_host_ms)), median(&all, |launch| launch.remote_unit_ms), median(&all, |launch| launch.client_ms)));
    }
    Ok(())
}

fn line(launch: &Launch) -> String {
    let after = |end: Option<u64>| end.map(|end| end.saturating_sub(launch.registered_ms));
    let registered = Timestamp::from_unix_millis(i64::try_from(launch.registered_ms).unwrap_or(i64::MAX));
    format!("{} registered {}: {}", launch.session, registered.local_twelve_hour(), offsets(after(Some(launch.session_host_ms)), after(launch.remote_unit_ms), after(launch.client_ms)))
}

fn offsets(session_host: Option<u64>, remote_unit: Option<u64>, client: Option<u64>) -> String {
    let shown = |ms: Option<u64>| ms.map_or_else(|| "-".to_owned(), |ms| format!("+{ms} ms"));
    format!("session host {}, remote unit {}, client's first line {}", shown(session_host), shown(remote_unit), shown(client))
}

fn median(all: &[Launch], end: impl Fn(&Launch) -> Option<u64>) -> Option<u64> {
    let mut spans: Vec<u64> = all.iter().filter_map(|launch| end(launch).map(|end| end.saturating_sub(launch.registered_ms))).collect();
    spans.sort_unstable();
    spans.get(spans.len() / 2).copied()
}
