//! Each kind of stop set by hand, read from the record its own command writes.
use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::{Hold, Lift, bounded};

const STABLE_PIN: &str = ".local/state/north/channel/stable";
const ASK: Duration = Duration::from_secs(10);

fn modified(path: &Path) -> Option<i64> {
    let at = fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(at.as_secs()).ok()
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

pub(super) fn review_holds(home: &Path) -> Vec<Hold> {
    let environment: Vec<(String, String)> = ["NORTH_VERDICT_CACHE_DIR", "XDG_STATE_HOME"].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
    let Ok(standing) = north_worker_sandbox::hold_ledger::standing(&environment, home) else { return Vec::new() };
    standing.into_iter().map(review_hold).collect()
}

fn review_hold(hold: north_worker_sandbox::hold_ledger::Standing) -> Hold {
    let on = hold.host.as_deref().map_or(String::new(), |host| format!(", on {host}"));
    Hold {
        host: String::new(),
        kind: format!("review hold on {}", hold.commit),
        set_by: hold.session.clone(),
        why: format!("{} (lane {}{on})", hold.why, hold.lane),
        since: i64::try_from(hold.at).ok(),
        lifts: Lift::Named(format!("`north runtime hold --lift {} --why TEXT` from {}{}", hold.commit, hold.session, hold.until.as_deref().map_or(String::new(), |until| format!(", or a landing of {until} reaching the screened head that changes a file of {} and whose patch {} accepted", hold.commit, hold.session)))),
        lifts_when: String::new(),
        red: None,
    }
}
