//! `north fleet hops NAME...`: where each named session's starts spent their time, read from the
//! launch trace its own processes wrote, one line per start and the medians across them.
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::hops::{COMMAND, Hop, Launch, launches, trace_in};
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;

type End = fn(&Launch) -> Option<u64>;

const ENDS: [(&str, End); 6] = [
    ("host open", |launch| launch.host_open_ms),
    ("session host", |launch| Some(launch.session_host_ms)),
    ("remote unit", |launch| launch.remote_unit_ms),
    ("client's first line", |launch| launch.client_ms),
    ("first user line", |launch| launch.prompt_ms),
    ("first answer", |launch| launch.response_ms),
];

pub(super) fn run(fleet: &Fleet, names: &[&str]) -> NorthResult<()> {
    if names.is_empty() {
        return Err(NorthError::Usage("Usage: north fleet hops NAME...".into()));
    }
    let mut all = Vec::new();
    for name in names {
        let found = traced(fleet, name)?;
        if found.is_empty() {
            println!("{name}: no launch trace");
        }
        for launch in &found {
            let registered = Timestamp::from_unix_millis(i64::try_from(launch.registered_ms).unwrap_or(i64::MAX));
            println!("{} registered {}: {}", launch.session, registered.local_twelve_hour(), line(std::slice::from_ref(launch)));
        }
        all.extend(found);
    }
    if all.len() > 1 {
        println!("median of {} starts: {}", all.len(), line(&all));
    }
    Ok(())
}

fn traced(fleet: &Fleet, name: &str) -> NorthResult<Vec<Launch>> {
    let live = fleet.session_dir(name)?;
    let retired = || fleet.retired_session(name).map(|(dir, _)| dir);
    let dirs: Vec<PathBuf> = [Some(live), retired()].into_iter().flatten().collect();
    if !dirs.iter().any(|dir| dir.exists()) {
        return Err(NorthError::Usage(format!("no session named {name}")));
    }
    Ok(dirs.iter().map(|dir| launches(&trace_in(dir))).find(|found| !found.is_empty()).unwrap_or_default())
}

fn line(starts: &[Launch]) -> String {
    let asked = median(starts, |launch| launch.asked_ms.map(|asked| launch.registered_ms.saturating_sub(asked)));
    let command: Vec<String> = COMMAND
        .iter()
        .map(|&hop| {
            let ms = median(starts, |launch| launch.command.iter().find(|(seen, _)| *seen == hop).map(|&(_, ms)| ms));
            format!("{} {}", label(hop), ms.map_or_else(|| "-".to_owned(), |ms| format!("{ms} ms")))
        })
        .collect();
    let mut parts = vec![asked.map_or_else(|| "launch command -".to_owned(), |ms| format!("launch command {ms} ms before ({})", command.join(", ")))];
    parts.extend(ENDS.iter().map(|(label, end)| {
        let after = median(starts, |launch| end(launch).map(|end| end.saturating_sub(launch.registered_ms)));
        after.map_or_else(|| format!("{label} -"), |ms| format!("{label} +{ms} ms"))
    }));
    parts.join(", ")
}

fn label(hop: Hop) -> &'static str {
    match hop {
        Hop::Startup => "startup",
        Hop::Admission => "admission",
        Hop::Placement => "placement",
        Hop::Gating => "gating",
        Hop::Prelude => "prelude",
        Hop::Launch | Hop::HostOpen | Hop::SessionHost | Hop::RemoteUnit | Hop::Client | Hop::Prompt | Hop::Response => "",
    }
}

fn median(starts: &[Launch], measure: impl Fn(&Launch) -> Option<u64>) -> Option<u64> {
    let mut values: Vec<u64> = starts.iter().filter_map(measure).collect();
    values.sort_unstable();
    values.get(values.len() / 2).copied()
}
