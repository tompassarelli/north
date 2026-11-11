//! The stop the proxy records for north fleet dispatch on a host, under `~/.local/state/north`: while it exists dispatch
//! launches nothing and the timer is meant stopped. It names its cause, who set it and what lifts it: a check command whose
//! exit 0 means the cause is gone, an expiry time, or both, so a stop that outlives its reason ends on its own (north#1136).
use std::path::{Path, PathBuf};

use north_foundation::foundation::clock::Timestamp;
use serde::{Deserialize, Serialize};

pub const RECORD: &str = "dispatch-paused";
pub const TIMER: &str = "north-dispatch.timer";
const UNRECORDED: &str = "unrecorded";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pause {
    pub reason: String,
    pub set_by: String,
    pub since: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
}

impl Pause {
    pub fn set(reason: &str, set_by: &str, until: Option<&str>, check: Option<&str>, since: Timestamp) -> Result<Self, String> {
        let (reason, set_by) = (reason.trim(), set_by.trim());
        if reason.is_empty() || set_by.is_empty() {
            return Err("a dispatch pause names its reason and who set it".into());
        }
        if let Some(text) = until {
            Timestamp::parse_rfc3339(text).ok_or_else(|| format!("--until {text:?} is not an RFC 3339 time"))?;
        }
        let check = check.map(str::trim).filter(|check| !check.is_empty());
        if until.is_none() && check.is_none() {
            return Err("a dispatch pause needs a cause check (--check COMMAND) or an expiry (--until TIME), so something lifts it".into());
        }
        Ok(Self { reason: reason.to_owned(), set_by: set_by.to_owned(), since: since.iso(), until: until.map(str::to_owned), check: check.map(str::to_owned) })
    }

    /// The reason the pause lifts at `now` (unix seconds): its cause check passed, else its expiry came.
    pub fn lifted_by(&self, now: f64, check_passes: impl FnOnce(&str) -> bool) -> Option<String> {
        if let Some(check) = self.check.as_deref().filter(|check| check_passes(check)) {
            return Some(format!("its cause check `{check}` exited 0"));
        }
        let until = self.until.as_deref().and_then(Timestamp::parse_rfc3339)?;
        (until.unix_seconds_f64() <= now).then(|| format!("it reached its expiry {}", until.iso()))
    }

    /// A record written as a bare reason before pauses carried a cause: it names no lift, so it stands until resumed.
    fn unrecorded(text: &str) -> Self {
        Self { reason: text.trim().to_owned(), set_by: UNRECORDED.to_owned(), since: String::new(), until: None, check: None }
    }

    pub fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        Some(serde_json::from_str(&text).unwrap_or_else(|_| Self::unrecorded(&text)))
    }

    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        path.parent().map_or(Ok(()), std::fs::create_dir_all)?;
        std::fs::write(path, text)
    }
}

pub fn record_path(home: &Path) -> PathBuf {
    home.join(".local/state/north").join(RECORD)
}

/// Why dispatch is paused on the host whose home is `home`, while the proxy's pause stands.
pub fn standing(home: &Path) -> Option<String> {
    Pause::read(&record_path(home)).map(|pause| pause.reason)
}

pub fn held_by(unit: &str) -> bool {
    let skip = format!("!%h/.local/state/north/{RECORD}");
    unit.lines().filter_map(|line| line.trim().strip_prefix("ConditionPathExists=")).any(|path| path.trim() == skip)
}
