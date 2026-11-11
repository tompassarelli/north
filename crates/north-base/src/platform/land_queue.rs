use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

pub const ROUND: &str = "NORTH_LAND_ROUND";
pub const TEST_ROOT: &str = "NORTH_TEST_ROOT";
const GITHUB_SSH: [&str; 2] = ["git@github.com:", "ssh://git@github.com/"];

pub fn test_root() -> Option<PathBuf> {
    std::env::var_os(TEST_ROOT).filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub fn confined(path: &Path) -> Result<(), String> {
    confined_in(path, test_root().as_deref())
}

pub fn confined_in(path: &Path, root: Option<&Path>) -> Result<(), String> {
    match root {
        Some(root) if !path.starts_with(root) => Err(format!("{} is outside the test fixture root {}; refusing to write there", path.display(), root.display())),
        _ => Ok(()),
    }
}

/// The queue a repository's landings wait in, keyed by its GitHub identity rather than the spelling of its origin URL, so hosts that clone it over ssh and over https share one queue.
pub fn key(origin: &str) -> String {
    Sha256::digest(canonical(origin).as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

fn canonical(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    let origin = origin.strip_suffix(".git").unwrap_or(origin);
    match GITHUB_SSH.iter().find_map(|prefix| origin.strip_prefix(prefix)) {
        Some(path) => format!("https://github.com/{path}.git"),
        None => format!("{origin}.git"),
    }
}

pub fn slug(branch: &str) -> String {
    branch.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') { character } else { '-' }).collect()
}

pub fn directory(state_home: &Path, origin: &str) -> PathBuf {
    queues(state_home).join(key(origin))
}

/// The directory holding every repository's landing queue on this host, one per repository key.
pub fn queues(state_home: &Path) -> PathBuf {
    state_home.join("north/land-queue")
}

/// The host name a ticket records as the host its worktree lives on.
pub fn local_host() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}

pub const OUTBOX: &str = "outbox";
pub const DECIDED: &str = "decided";

/// A lane the lander landed from a host: the worktree its ticket names, the head the lander took and the commit it made on main.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LandedLane {
    pub root: PathBuf,
    pub head: String,
    pub commit: String,
}

/// The landed tickets from `host` in each of `queues`: its decided area and the verdicts the lander sent back to its outbox.
pub fn landed_lanes(queues: &[PathBuf], host: &str) -> Vec<LandedLane> {
    let areas = queues.iter().flat_map(|queue| [queue.join("done"), queue.join(OUTBOX).join(DECIDED)]);
    let landed = |ticket: &Path| {
        let ticket = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(ticket).ok()?).ok()?;
        let commit = ticket["verdict"].as_str()?.strip_prefix("landed ")?.split_whitespace().next()?.to_owned();
        (ticket["host"].as_str() == Some(host)).then_some(())?;
        Some(LandedLane { root: PathBuf::from(ticket["root"].as_str()?), head: ticket["head"].as_str()?.to_owned(), commit })
    };
    let mut lanes: Vec<LandedLane> = areas
        .flat_map(|area| fs::read_dir(area).into_iter().flatten().flatten().map(|entry| entry.path()))
        .filter(|ticket| ticket.extension().is_some_and(|extension| extension == "json"))
        .filter_map(|ticket| landed(&ticket))
        .collect();
    lanes.sort();
    lanes.dedup();
    lanes
}

/// The undecided tickets whose lane is the worktree at `root` on `host`, in any queue under `queues` or its outbox:
/// ticket files no lander has yet decided, withdrawn or taken from the outbox, so the worktree must stay until each is.
/// Every host lays its lanes out alike, so a ticket from another host never holds a worktree here.
pub fn waiting_on(queues: &Path, host: &str, root: &Path) -> Vec<PathBuf> {
    let canonical = fs::canonicalize(root).ok();
    let names_root = |named: &Path| named == root || canonical.is_some() && fs::canonicalize(named).ok() == canonical;
    let lane_of = |ticket: &Path| {
        let ticket = serde_json::from_str::<serde_json::Value>(&fs::read_to_string(ticket).ok()?).ok()?;
        ticket["host"].as_str().is_none_or(|named| named == host).then(|| ticket["root"].as_str().map(PathBuf::from)).flatten()
    };
    fs::read_dir(queues)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|queue| [queue.path(), queue.path().join("outbox")])
        .flat_map(|directory| fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()))
        .filter(|ticket| ticket.extension().is_some_and(|extension| extension == "json") && ticket.is_file())
        .filter(|ticket| lane_of(ticket).is_some_and(|named| names_root(&named)))
        .collect()
}

/// The fleet sessions whose lane waits undecided in any repository's queue under `state_home`: a ticket the lander has
/// neither moved to `done/` nor withdrawn.
pub fn queued_sessions(state_home: &Path) -> BTreeSet<String> {
    let queues = fs::read_dir(queues(state_home)).into_iter().flatten().flatten().map(|queue| queue.path());
    queues
        .flat_map(|queue| fs::read_dir(&queue).into_iter().flatten().flatten().map(move |ticket| (queue.clone(), ticket.path())))
        .filter(|(queue, ticket)| ticket.extension().is_some_and(|extension| extension == "json") && ticket.is_file() && !ticket.file_name().is_some_and(|name| queue.join("withdrawn").join(name).is_file()))
        .map(|(_, ticket)| ticket)
        .filter_map(|ticket| serde_json::from_str::<serde_json::Value>(&fs::read_to_string(ticket).ok()?).ok()?["session"].as_str().map(str::to_owned))
        .collect()
}

/// Seconds since a ticket was queued, read from its `queued_at` field, else the file's mtime; never from the file-name id, which jump-the-queue tickets set deliberately old.
pub fn ticket_age(ticket: &Path, now: f64) -> Option<f64> {
    let queued = fs::read_to_string(ticket).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|ticket| ticket["queued_at"].as_str().and_then(rfc3339_seconds));
    let at = queued.or_else(|| fs::metadata(ticket).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|since| since.as_secs_f64()))?;
    Some((now - at).max(0.0))
}

pub fn rfc3339_seconds(text: &str) -> Option<f64> {
    let (date, clock) = text.strip_suffix('Z')?.split_once('T')?;
    let mut day = date.splitn(3, '-').map(str::parse::<i32>);
    let (year, month, day) = (day.next()?.ok()?, day.next()?.ok()?, day.next()?.ok()?);
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut parts = whole.splitn(3, ':').map(str::parse::<u8>);
    let (hour, minute, second) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    let date = time::Date::from_calendar_date(year, time::Month::try_from(u8::try_from(month).ok()?).ok()?, u8::try_from(day).ok()?).ok()?;
    let at = date.with_hms(hour, minute, second).ok()?.assume_utc().unix_timestamp() as f64;
    let fraction = if fraction.is_empty() { 0.0 } else { format!("0.{fraction}").parse::<f64>().ok()? };
    Some(at + fraction)
}

/// The user unit that serves a checkout's queue, `north-land@PROJECT.service`: PROJECT is the directory above a `main` checkout, else the checkout's own name.
pub fn lander_unit(checkout: &Path) -> String {
    let project = if checkout.file_name().is_some_and(|name| name == "main") { checkout.parent().and_then(Path::file_name) } else { checkout.file_name() };
    format!("north-land@{}.service", project.unwrap_or_default().to_string_lossy())
}

pub const REST_UNIT: &str = "north-land-rest.service";

fn unit_declared(unit: &str) -> bool {
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    config.is_some_and(|config| config.join("systemd/user").join(unit).exists())
}

/// Whether this host declares the checkout's own lander: nixos-config's north-land module writes one unit file per repository it lists into the user's systemd directory.
pub fn lander_declared(checkout: &Path) -> bool {
    unit_declared(&lander_unit(checkout))
}

/// The declared unit that serves a checkout's queue on this host: its own lander, else `north-land-rest`, which serves every queue no own lander does.
pub fn serving_unit(checkout: &Path) -> Option<String> {
    let own = lander_unit(checkout);
    if unit_declared(&own) { Some(own) } else { unit_declared(REST_UNIT).then(|| REST_UNIT.to_owned()) }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::confined_in;

    #[test]
    fn a_path_outside_the_declared_fixture_root_is_refused() {
        assert!(confined_in(Path::new("/fixture/state"), Some(Path::new("/fixture"))).is_ok());
        assert!(confined_in(Path::new("/home/tom/.local/state"), Some(Path::new("/fixture"))).is_err());
        assert!(confined_in(Path::new("/home/tom/.local/state"), None).is_ok());
    }
}
