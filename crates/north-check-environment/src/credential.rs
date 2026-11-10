//! A red run whose log shows GitHub refusing a credential or a sibling repository failing to fetch is the host's
//! failure, not a lane's: the same refusal reds every lane, so no lane is reverted for it (north#818).
use std::fs;
use std::path::Path;

const MARKS: [&str; 4] = ["told us to quit", "could not read Username", "Authentication failed", "outside its 55 min trust"];

/// The first line of `log` that shows a refused GitHub credential or a failed fetch of a sibling repository's origin/main.
pub fn refusal(log: &str) -> Option<String> {
    let refused = |line: &&str| MARKS.iter().any(|mark| line.contains(mark)) || (line.contains("cannot fetch ") && line.contains(" origin/main"));
    log.lines().find(refused).map(|line| line.trim().chars().take(200).collect())
}

/// The verdict that reverts nothing when the log at `log` shows such a refusal or a ref-lock failure, None when it is unreadable or shows none.
pub fn incident(log: &Path) -> Option<String> {
    let text = fs::read_to_string(log).ok()?;
    refusal(&text).or_else(|| north_base::foundation::check_log::ref_lock(&text)).map(|line| format!("infrastructure incident, not a lane's failure ({line}); nothing is reverted"))
}
