//! Each kind of stop set by hand, read from the record its own command writes.
use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde::Deserialize;

use super::{Hold, Lift, bounded};
use crate::foundation::clock::Timestamp;

const STABLE_PIN: &str = ".local/state/north/channel/stable";
const AUTOMATIC_LAPTOP_WORDS: [&str; 3] = ["window", "laptop tick", "laptop-auto"];
const ASK: Duration = Duration::from_secs(10);

fn modified(path: &Path) -> Option<i64> {
    let at = fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(at.as_secs()).ok()
}

fn at(text: &str) -> Option<i64> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds)
}

pub(super) fn dispatch_pause(home: &Path) -> Option<Hold> {
    let (pause, path) = crate::dispatch::recorded_pause(home)?;
    let lifts = match (pause.check, pause.until.as_deref().and_then(at)) {
        (Some(check), Some(until)) => Lift::CheckOrUntil(check, until),
        (Some(check), None) => Lift::Check(check),
        (None, Some(until)) => Lift::Until(until),
        (None, None) => Lift::Unnamed("nothing recorded: it stands until `north fleet dispatch resume`".into()),
    };
    Some(Hold { host: String::new(), kind: "dispatch pause".into(), set_by: pause.set_by, why: pause.reason, since: at(&pause.since).or_else(|| modified(&path)), lifts, lifts_when: String::new(), red: None })
}

pub(super) fn stable_pin(home: &Path) -> Option<Hold> {
    let path = home.join(STABLE_PIN);
    let pinned = fs::read_to_string(&path).ok()?;
    let sha = pinned.trim();
    let short = sha.get(..10).unwrap_or(sha);
    Some(Hold {
        host: String::new(),
        kind: "stable-pin hold".into(),
        set_by: "not recorded (`stable-pin hold`)".into(),
        why: format!("the runtime follow timers stay on {short} while the pin file stands, and on the laptop north-stable-hold.service keeps the promotion lock"),
        since: modified(&path),
        lifts: Lift::Named(format!("`stable-pin release` removes {STABLE_PIN} and stops north-stable-hold.service; `north runtime order` moves the pin to the ordered commit")),
        lifts_when: String::new(),
        red: None,
    })
}

pub(super) fn unit_description(unit: &str) -> Option<String> {
    let said = bounded("systemctl", &["--user", "show", unit, "-p", "Description", "--value"], ASK)?;
    Some(said.trim().to_owned())
}

pub(super) fn session_holds(now: i64) -> Vec<Hold> {
    let Ok(root) = crate::fleet::registry::root_here() else { return Vec::new() };
    let mut holds = Vec::new();
    for entry in fs::read_dir(&root).into_iter().flatten().flatten().filter(|entry| !entry.file_name().to_string_lossy().starts_with('.')) {
        let Ok(record) = crate::fleet::records::read_record(&entry.path()) else { continue };
        if record.extra_tasks.is_empty() {
            continue;
        }
        let ended = record.ended().then(|| modified(&crate::fleet::records::record_file(&entry.path()))).flatten();
        for task in &record.extra_tasks {
            holds.push(Hold {
                host: String::new(),
                kind: format!("issue hold on {task}"),
                set_by: record.name.clone(),
                why: format!("{} holds it beside its own task, so dispatch staffs no one else on it", record.name),
                since: i64::try_from(record.created_ms / 1000).ok(),
                lifts: Lift::Named(format!("`north fleet hold --release {task}` from {}, or that session ends and is retired", record.name)),
                lifts_when: String::new(),
                red: ended.filter(|ended| now - ended >= super::OUTLIVED_SECS).map(|_| format!("{} has ended and it still stands", record.name)),
            });
        }
    }
    holds
}

#[derive(Deserialize)]
struct LaptopWord {
    state: String,
    since: String,
    reason: String,
    by: String,
    #[serde(default)]
    until: Option<String>,
}

/// The laptop's drain or close word, when a person or session set it; window edges and laptop-auto observations are not set by hand.
pub(super) fn laptop_word() -> Option<Hold> {
    let said = crate::laptop::Ctx::from_env().main(&["status".to_owned(), "--json".to_owned()]).ok()?;
    let word: LaptopWord = serde_json::from_str(said.trim()).ok()?;
    if word.state == "available" || AUTOMATIC_LAPTOP_WORDS.contains(&word.by.as_str()) {
        return None;
    }
    let lifts = word.until.as_deref().and_then(|until| at(until).or_else(|| Timestamp::parse(until).ok().map(Timestamp::unix_seconds))).map_or_else(|| Lift::Named("the next window edge, or `north fleet laptop open`".into()), Lift::Until);
    Some(Hold { host: "laptop".into(), kind: format!("laptop drain switch ({})", word.state), set_by: word.by, why: word.reason, since: at(&word.since).or_else(|| Timestamp::parse(&word.since).ok().map(Timestamp::unix_seconds)), lifts, lifts_when: String::new(), red: None })
}
