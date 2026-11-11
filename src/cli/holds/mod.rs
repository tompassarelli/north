//! `north holds list [--here] [--json]`: every stop a person or the coordinator set by hand, on this host and every host it
//! ships runtimes to, with who set it, why, since when and what lifts it; one that outlived its lift by 10 minutes, or names
//! nothing that lifts it, is red (north#1136).
mod locks;
mod sources;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north holds list [--here] [--json]";
const OUTLIVED_SECS: i64 = 600;
const ASK_SECS: &str = "20";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum Lift {
    Check(String),
    Until(i64),
    CheckOrUntil(String, i64),
    Named(String),
    Unnamed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    #[serde(default)]
    pub host: String,
    pub kind: String,
    pub set_by: String,
    pub why: String,
    pub since: Option<i64>,
    pub lifts: Lift,
    #[serde(default)]
    pub lifts_when: String,
    pub red: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Listing {
    holds: Vec<Hold>,
    unread: Vec<(String, String)>,
}

impl Lift {
    fn text(&self) -> String {
        let at = |seconds: i64| Timestamp::from_unix_seconds(seconds).local_twelve_hour();
        match self {
            Self::Check(check) => format!("its cause check `{check}` exits 0"),
            Self::Until(until) => format!("its expiry at {}", at(*until)),
            Self::CheckOrUntil(check, until) => format!("its cause check `{check}` exits 0, or its expiry at {}", at(*until)),
            Self::Named(text) | Self::Unnamed(text) => text.clone(),
        }
    }
}

impl Hold {
    /// Marks the hold red when it names nothing that lifts it, or its lift came 10 minutes ago and it still stands.
    fn judged(mut self, now: i64, check_passes: &dyn Fn(&str) -> bool) -> Self {
        let stood = self.since.is_some_and(|since| now - since >= OUTLIVED_SECS);
        let expired = |until: i64| now - until >= OUTLIVED_SECS;
        self.lifts_when = self.lifts.text();
        self.red = match &self.lifts {
            Lift::Unnamed(_) if stood || self.since.is_none() => Some("it names nothing that lifts it".into()),
            Lift::Until(until) | Lift::CheckOrUntil(_, until) if expired(*until) => Some("its expiry passed over 10 minutes ago and it still stands".into()),
            Lift::Check(check) | Lift::CheckOrUntil(check, _) if stood && check_passes(check) => Some("its cause check passes and it still stands".into()),
            _ => self.red.take(),
        };
        self
    }

    fn line(&self) -> String {
        let since = self.since.map_or_else(|| "since unknown".to_owned(), |since| format!("since {}", Timestamp::from_unix_seconds(since).local_twelve_hour()));
        let red = self.red.as_deref().map(|why| format!("  RED: {why}")).unwrap_or_default();
        format!("{} {}: {}\n  set by {}, {since}\n  lifts when: {}{red}", self.host, self.kind, self.why, self.set_by, self.lifts.text())
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn here() -> Vec<Hold> {
    let (home, now, host) = (home(), Timestamp::now().unix_seconds(), crate::platform::host::name());
    let mut holds: Vec<Hold> = sources::dispatch_pause(&home).into_iter().collect();
    holds.extend(sources::stable_pin(&home));
    holds.extend(locks::holds(&home.join(".local/state/north"), std::path::Path::new("/proc"), sources::unit_description));
    holds.extend(sources::session_holds(now));
    holds.into_iter().map(|hold| Hold { host: host.clone(), ..hold.judged(now, &crate::dispatch::pause_check_passes) }).collect()
}

fn asked(destination: &str) -> Result<Vec<Hold>, String> {
    let output = Command::new("timeout")
        .args([ASK_SECS, "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north", "holds", "list", "--here", "--json"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{}: {}", output.status, said.lines().last().unwrap_or_default().trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("answered no hold list: {error}"))
}

fn fleet_wide() -> Listing {
    let now = Timestamp::now().unix_seconds();
    let others = std::thread::scope(|scope| {
        let asking: Vec<_> = crate::land_offload::other_destinations().into_iter().map(|destination| scope.spawn(move || (asked(&destination), destination))).collect();
        asking.into_iter().filter_map(|asking| asking.join().ok()).collect::<Vec<_>>()
    });
    let mut listing = Listing { holds: here(), unread: Vec::new() };
    listing.holds.extend(sources::laptop_word().into_iter().map(|hold| hold.judged(now, &|_| false)));
    for (answer, destination) in others {
        match answer {
            Ok(holds) => listing.holds.extend(holds),
            Err(why) => listing.unread.push((destination, why)),
        }
    }
    listing
}

pub fn run(words: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    let (here_only, json) = match words.as_slice() {
        ["list"] => (false, false),
        ["list", "--json"] => (false, true),
        ["list", "--here"] => (true, false),
        ["list", "--here", "--json"] => (true, true),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let listing = if here_only { Listing { holds: here(), unread: Vec::new() } } else { fleet_wide() };
    match (json, here_only) {
        (true, true) => println!("{}", serde_json::to_string(&listing.holds)?),
        (true, false) => println!("{}", serde_json::to_string(&listing)?),
        (false, _) => {
            if listing.holds.is_empty() {
                println!("no stop set by hand stands");
            }
            listing.holds.iter().for_each(|hold| println!("{}", hold.line()));
            listing.unread.iter().for_each(|(host, why)| println!("{host}: holds not read: {why}"));
        }
    }
    Ok(())
}

pub(super) fn bounded(program: &str, arguments: &[&str], limit: Duration) -> Option<String> {
    let seconds = limit.as_secs().to_string();
    let output = Command::new("timeout").arg(seconds).arg(program).args(arguments).stdin(std::process::Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
